Dray is a native, open-source desktop client for Google Drive, written in Rust. The brand says one thing: fast and out of your way. Dense, quiet, warm, and never decorated for its own sake.

## Content fundamentals

- Write like a terse colleague: short, concrete, no hype. Sentence case everywhere, buttons and menus included ("New folder", "Copy link"). The only capitalized word is the name, Dray.
- Name the object, not the user: "3 files selected", not "You have selected 3 files".
- Say what happened and what Dray does next: "Couldn't reach Google. Retrying in 5 s." Never "Oops" or "Something went wrong".
- Set numbers and units in `mono`, with a space before the unit: "4.2 MB", "1,204 files", "5 s".
- No emoji, no exclamation marks, no mascots, no jokes in error states.
- Write "Google Drive" only to name the service Dray connects to, and carry the line "Dray is an independent project and is not affiliated with or endorsed by Google." on the site and in About. Never put Google or Drive's logo, name or four-color palette into Dray's own identity.

Examples: "Synced 1,204 files in 3.1 s." / "Offline. Changes will upload when you reconnect." / "Delete 12 files? They stay in Trash for 30 days."

## Name and mark

- The name is Dray (a dray is a low cart for hauling heavy loads). Always "Dray"; never "DRAY". The command-line binary is `dray`, in `mono`.
- The mark is a folder whose body is a D, with a forward chevron cut out of it: file, name and speed in one shape. Use `mark-copper.svg` on `surface` and `surface-raised`, `mark-paper.svg` on `brand`, `ink` and dark photography, `mark-ink.svg` for one-color print. `app-icon.svg` is the only version with a background.
- Lockup: mark at 1.2 times the name's cap height, `space-2` gap, name in `display-xl` or `display` at weight 700. Clear space around it is half the mark's width. Minimum mark size is 16 px.
- Do not recolor the mark outside the three provided inks, outline it, rotate it, add shadows, or set the name in another face.

## Color

- Neutrals carry the interface. Use `surface` for the window, `surface-raised` for the file list and dialogs, `surface-sunken` for the sidebar, toolbar wells and hover rows. Text is `ink`, secondary text `ink-muted`.
- Copper (`brand`) is scarce: one filled primary action per view, the mark, the folder icon, links, and the selection tint `brand-subtle`. Text on a brand fill is `on-brand`. Use `brand` as text only on `surface`, `surface-raised` and `brand-subtle`.
- Verdigris (`patina`) is the second voice: progress fills, `focus-ring`, success. Never put patina and brand at equal weight in the same control.
- Status colors (`success`, `info`, `warning`, `danger`) always come with a word or an icon, never hue alone. Prefer blue (`info`) and amber over red and green pairs.
- Both themes ship. Dark follows the OS. Never invert with a filter; use the dark values.
- Hairlines between panes and rows use `border`. Anything a user must find as a control (inputs, checkboxes, secondary buttons) uses `border-strong`.
- Focus: 2 px solid `focus-ring`, 2 px offset, on every focusable element.

## Type

- The app's interface uses the system UI font (the `ui` family, styles `ui-title`, `ui-row`, `ui-label`, `ui-small`). A native app looks native.
- Schibsted Grotesk (`sans`) is for the name, the website, docs and README art: `display-xl`, `display`, `title`, `body`, `small`. Hosted by Google Fonts; no font files are bundled.
- JetBrains Mono (`mono`, `mono-small`) is for sizes, dates, paths, shortcuts, logs and the CLI. Use it for any value a person might copy. The app bundles JetBrains Mono Regular (OFL 1.1, `assets/fonts/`).
- Interface text sits between 12 and 15 px. File names are `ui-row`, 13 px.

## Space, shape and depth

- Everything sits on a 4 px grid: use `space-*` tokens. File rows are `row-default` (36 px) or `row-compact` (28 px).
- Radii are tight: `radius-md` for buttons, inputs and rows; `radius-sm` for tags and key hints; `radius-lg` for menus, popovers and dialogs.
- The brand shape is the D: a square left edge and a fully rounded right end (`radius-full`). Progress bars, status pills and decorative bars use it. Do not round both ends of a bar.
- Separate panes with `border`, not shadow. `shadow-popover` is only for floating layers.

## Motion

- Hover and press change color in 100 ms ease-out; panels slide in 200 ms. Nothing animates on scroll or on list load. Honor reduced motion.
- Progress is real or absent: never show a determinate bar that is not tied to bytes or files.

## Iconography

- Icons are Lucide (ISC license), bundled as SVGs in `assets/icons/`. Use 16 px icons (20 px in the toolbar), 1.5 px stroke, round caps and joins, `ink-muted` at rest and `ink` when selected.
- The folder icon is the only icon colored `brand`. File types are told apart by shape and label, not by hue.
- No emoji, and no Google Drive file-type icons.

## Imagery

- Show the real app, in the host OS's window chrome, on a `surface-sunken` backdrop. No stock photos or people illustrations.
- Decorative graphics are built from D-ended bars and dot trails in `brand`, `ink` and `patina`, as on the cover.

## Using this system

- Components here are reference previews, not a shipped bundle: `Button`, `FileRow`, `Status`, `Logo`. Recreate them in the Rust UI toolkit using the token values; `tokens.json` is the source of truth for theme constants.
- Logo files are in `logo/`; its README states the ink of each file.
