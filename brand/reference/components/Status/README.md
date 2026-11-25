Status pills report sync and transfer state in the status bar, in file rows and in dialogs, always as an icon plus a word.

The consumer provides one of four states: Synced (`success`), Syncing (`info`), Offline (`warning`), Failed (`danger`). The pill is 24 px tall, `ui-small` at weight 600, a 1 px edge and text in the state color on `surface-raised`, with a square left corner (`radius-sm`) and a fully rounded right end (`radius-full`) echoing the mark. State color text is used on `surface` and `surface-raised` only.

Do not rely on color alone, add new states without an icon and a word, or fill the pill with the state color.
