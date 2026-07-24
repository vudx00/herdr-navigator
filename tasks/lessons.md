# Lessons

## Verify keybindings through the terminal host

A TUI unit test only proves the application handles a key event after receiving it. Herdr evaluates direct host bindings such as `focus_pane_down = "alt+j"` before forwarding input to normal split, tab, zoomed, or overlay panes, so an inner Alt-J/Alt-K binding cannot override those host actions.

For keybinding changes inside a multiplexer-managed UI:

1. Inspect the host's input-routing precedence and the user's active host bindings.
2. Verify the chosen pane type actually forwards the key before host dispatch.
3. Exercise the installed plugin end to end; do not treat an inner key-handler test as sufficient proof.

Herdr plugin popups are the supported modal surface for this case: they receive all terminal input before global direct bindings are evaluated. Popup plugin panes require Herdr 0.7.4 or newer.
