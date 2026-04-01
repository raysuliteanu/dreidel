# [MED] View/state enum cloned just to pattern-match, in key handler and draw paths

## Location
- `src/components/cpu.rs:258` — `match self.state.clone()`
- `src/components/net.rs:139` — `match &self.view.clone()`
- `src/components/disk.rs:141` — `match &self.view.clone()`
- `src/components/process/mod.rs:153, 375, 412` — similar patterns

## Description
The view/state enum is cloned at the start of `handle_key_event` (and in some draw helpers)
so that the match arm can destructure owned `String` fields (e.g. `Filter { input: String }`)
while `self` remains available for mutation inside the arm.

```rust
// Example from net.rs:
match &self.view.clone() {
    NetView::Filter { input } => {
        // input: &String, borrowed from the clone
        let mut s = input.clone();  // second clone
        s.push(c);
        self.filter = s.clone();   // third clone
        self.view = NetView::Filter { input: s };
    }
    ...
}
```

The root pattern results in at least 3 allocations per keypress when in filter mode: one for
the view clone, one for the input string, and one for the filter field.

In the draw path (`match &self.view.clone()`), the clone is even less justified because draw
does not mutate `self.view` — matching on `&self.view` directly is sufficient there.

## Impact
- 1–3 unnecessary String clones per keypress while in filter mode.
- Unnecessary enum clone on every draw frame for `draw()` dispatch.

## Recommended Fix

**For draw dispatch** — remove `.clone()` entirely, match on `&self.view`:
```rust
fn draw(&mut self, frame: &mut Frame, area: Rect) -> Result<()> {
    match &self.view {
        NetView::List | NetView::Filter { .. } => self.draw_list(frame, area),
        NetView::Detail { name } => {
            let name = name.clone(); // clone only the name string, not the whole enum
            self.draw_detail(frame, area, &name)
        }
    }
}
```

**For key handler** — use `std::mem::replace` to take ownership without cloning:
```rust
if let ListView::Filter { input } = std::mem::replace(&mut self.view, ListView::List) {
    match key.code {
        KeyCode::Esc => {
            self.filter.clear();
            // view is already ListView::List from the replace
        }
        KeyCode::Char(c) => {
            let mut s = input;
            s.push(c);
            self.filter = s.clone();
            self.view = ListView::Filter { input: s };
        }
        ...
    }
}
```
