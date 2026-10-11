//! App-owned hub operations. Link handlers run these on workers; only native
//! hotkey registration is marshalled to the UI. Serialize settings mutations
//! across concurrent clients, including the separate settings process reload.
use crate::{
    hotkey, link, portable_settings,
    service::{Service, UiMsg},
};
use arcade_link::app::{ImportResult, ShortcutSetResult};
use arcade_link::{Content, LinkError};
use std::sync::Arc;

impl Service {
    fn publish_hub_settings(&self, settings: &find_core::Settings) -> Result<(), LinkError> {
        let doc = link::document(settings, &self.paths);
        if let Some(p) = self.presence() {
            p.update_document(doc.clone());
        } else {
            arcade_link::manifest::write_document(&self.locations, &doc).map_err(portable_settings::io_error)?;
        }
        // Presence's additive API returns (). Verify its write so an IPC caller
        // never receives success for a shortcut absent from the registry.
        let text = std::fs::read_to_string(self.locations.manifest(link::ME)).map_err(portable_settings::io_error)?;
        let mut written = arcade_link::manifest::ManifestDocument::from_json(&text).map_err(LinkError::unavailable)?;
        written.manifest.written_at = doc.manifest.written_at.clone();
        if written != doc {
            return Err(LinkError::unavailable("couldn't publish settings in the manifest"));
        }
        Ok(())
    }
    pub fn set_shortcut(self: &Arc<Self>, id: &str, input: &str) -> Result<ShortcutSetResult, LinkError> {
        if id != "show" {
            return Err(LinkError::denied("unknown shortcut id"));
        }
        let key = hotkey::validate(input)?;
        let _change = self.settings_changes.lock().unwrap_or_else(|e| e.into_inner());
        let old = self.settings();
        let state = hotkey::set_live(self, &key)?;
        let result = hotkey::result(&state)?;
        let mut new = old.clone();
        new.shortcut = key;
        if let Err(e) = new.save(&self.paths.settings_file()) {
            let _ = hotkey::set_live(self, &old.shortcut);
            return Err(portable_settings::io_error(e));
        }
        if let Err(e) = self.publish_hub_settings(&new) {
            let _ = old.save(&self.paths.settings_file());
            let _ = self.publish_hub_settings(&old);
            let _ = hotkey::set_live(self, &old.shortcut);
            return Err(e);
        }
        *self.settings.write().unwrap_or_else(|e| e.into_inner()) = new.clone();
        self.set_shortcut_state(state.to_json());
        Ok(result)
    }

    pub fn export_settings(&self) -> Result<Vec<Content>, LinkError> {
        let _change = self.settings_changes.lock().unwrap_or_else(|e| e.into_inner());
        let pins = self.pins.lock().unwrap_or_else(|e| e.into_inner()).clone();
        portable_settings::export_file(&self.locations, &self.settings(), &pins, &find_core::paths::home_dir())
    }

    pub fn import_settings(self: &Arc<Self>, inputs: &[Content]) -> Result<ImportResult, LinkError> {
        let doc = portable_settings::read_import(inputs)?;
        let _change = self.settings_changes.lock().unwrap_or_else(|e| e.into_inner());
        let old = self.settings();
        let (new, pins) = doc.apply_to(&old, &find_core::paths::home_dir());
        let state = hotkey::set_live(self, &new.shortcut)?;
        hotkey::result(&state)?;
        let backup = match portable_settings::save_import(&self.paths, &new, &pins) {
            Ok(backup) => backup,
            Err(e) => {
                let _ = hotkey::set_live(self, &old.shortcut);
                return Err(portable_settings::io_error(e));
            }
        };
        if let Err(e) = self.publish_hub_settings(&new) {
            let _ = portable_settings::restore_backup(&self.paths, &backup);
            let _ = self.publish_hub_settings(&old);
            let _ = hotkey::set_live(self, &old.shortcut);
            return Err(e);
        }
        *self.settings.write().unwrap_or_else(|e| e.into_inner()) = new.clone();
        *self.pins.lock().unwrap_or_else(|e| e.into_inner()) = pins;
        self.engine.set_settings(new.clone());
        self.set_shortcut_state(state.to_json());
        self.send(UiMsg::Settings);
        self.send(UiMsg::Refresh);
        Ok(ImportResult { imported: true, restart_required: false })
    }
}
