The file row is Dray's core list item: icon, name, owner or state, modified date and size, in a `surface-raised` list with `border` hairlines.

The consumer provides name, owner, modified time, size and sync state. Row height is `row-default` (36 px) or `row-compact` (28 px), padding `space-3`, names in `ui-row`, dates and sizes in `mono-small`, sizes right-aligned. Hover is `surface-sunken`; selection is `brand-subtle` with `ink` text and no accent bar. The folder icon is `brand`; file icons are `ink-muted` outlines. Progress uses a D-ended bar (square left, `radius-full` right) filled `patina`. A failed row shows the word "Failed" in `danger` and the retry time in `mono-small`.

Do not tell file types apart by color, show a progress bar that is not tied to real bytes, or use a left border to mark selection.
