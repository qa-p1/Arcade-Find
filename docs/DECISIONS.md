# Decisions log

| Date | Decision | Why |
|---|---|---|
| 2026-10-09 | **Enter previews in Arcade Look** (`look.preview`); Shift+Enter opens with the default app; Ctrl+Enter reveals; Space (after moving into the list) also previews. If Look is missing, disabled, or doesn't accept the item, Enter falls back to the default app. | Owner request: previewing is the most common action and should be the default. The fallback keeps the primary key working without peers (NEW_APP_SPEC §2). |
