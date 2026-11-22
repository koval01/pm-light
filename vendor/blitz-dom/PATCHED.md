# blitz-dom 0.3.0-beta.4, patched

The crate as published on crates.io, with three changes, each marked `PATCHED` in the source. Blitz draws an HTML
mail when the preferences choose it over the system's web view (`core/src/page.rs`, `docs/BLITZ.md`).

- `src/layout/table.rs`, `collect_table_cells`: a child of a table row that is a block, flex or grid box was taken
  for a caption and left out, with all its content. Newsletters make their cells blocks on narrow screens, under a
  media query, so that their columns stack: on a narrow pane such a mail lost its text and pictures. CSS wraps such a
  child in an anonymous cell; here it takes a row of the table's grid to itself, across every column, which stacks
  the columns as browsers do. `stack_block` and `is_block` are new.
- `src/layout/table.rs`: `colspan` is clamped to 1,000, as browsers clamp it. Unclamped, two cells spanning 40,000
  columns overflowed the column counter, a `u16`, and the slice taken with it panicked.
- `src/stylo.rs`, `parse_color_attr`: a colour attribute that is not ASCII is refused before it is read by byte
  offsets. `bgcolor="#aéé1"` cut a letter in two and panicked.

`core/src/html` keeps spans and colours within these bounds too, for the system's web view and in case this patch
is dropped. The hostile mails of `scripts/attacks.sh` (`core/src/page/tests/hostile.rs`) run against both.

Drop this patch once Blitz wraps such children in anonymous cells and bounds both attributes itself.
