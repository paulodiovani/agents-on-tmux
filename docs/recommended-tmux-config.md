## AOT Recommended TMUX Config

AOT works by running a nested Tmux session.

For better experience, use these settings to interact with the nested session from the parent.

### Send prefix to nested session

```tmux
bind-key C-b send-prefix
```

Then type `Ctrl + b` twice to run bindings in the nested session.
Update to your prefix, or preferred key.

### Focus events

```tmux
set-option -g focus-events on
```

Required for the TUI footer to switch keybinding hints by focus: while the
TUI pane is focused it shows the TUI's own keys (`c new`, `q quit`, ...);
when another pane is focused it shows your own tmux bindings to control the
nested session (e.g. `C-b C-b c new`, resolved from `list-keys`). Without
this option tmux does not report pane focus changes, and the footer always
shows the TUI keys.

### Detect Shift+Enter

```tmux
bind -n S-Enter send-keys Escape "[13;2u"
```

Binds `Shift + Enter` to its escape sequence, so it works on nested sessions.

### Tmux-nested key bindings

```tmux
# Tmux Nested
# from http://stahlke.org/dan/tmux-nested/

# go to nested tmux
bind -n M-S-up \
  set -q status "off" \; \
  set -q prefix C-a

# go to parent tmux
bind -n M-S-down \
  set -q status "on" \; \
  set -q prefix C-b
```

By typing `Shift + Alt + Up` you _go into_ the nested session, and by `Shift + Alt + Down` you _go back_ to parent.
This is a minimum config based on http://stahlke.org/dan/tmux-nested/ that hides the status bar instead of changing color.
Change the prefix and adjust to your needs.

### Window activity notification

```tmux
set-option -g monitor-activity on
set-option -g visual-activity off
set-option -g activity-action other
```

Marks windows with pending output from an inactive agent (`!` indicator in the TUI).
