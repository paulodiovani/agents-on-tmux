# AOT Landing Page

This directory contains the static landing page for Agents on TMUX, hosted on GitHub Pages.

## Structure

- `index.html` — Main landing page with embedded terminal demo
- `assets/demo.js` — xterm.js player for asciinema recordings
- `assets/demo.cast` — Asciinema recording (you need to create this)
- `recommended-tmux-config.md` — TMUX configuration guide (existing)

## Recording the Demo

The terminal demo uses an asciinema `.cast` file. To create it:

### 1. Install asciinema

```bash
# macOS
brew install asciinema

# Linux
sudo apt install asciinema
# or
pip install asciinema
```

### 2. Record the demo

```bash
# Record with fixed dimensions (matches the player)
asciinema rec --cols 120 --rows 38 docs/assets/demo.cast
```

### 3. Suggested demo workflow

Record yourself:
1. Starting with `aot` in a TMUX session
2. Show the TUI panel appearing on the left
3. Launch an agent (e.g., `claude` or `aider`)
4. Show agent detection in the TUI
5. Navigate between agents/windows
6. Show key bindings in action
7. Maybe rename a window or stop an agent

Keep it concise — 30-60 seconds is ideal.

### 4. Optimize the recording (optional)

You can edit the `.cast` file to:
- Remove long pauses
- Speed up boring parts
- Trim the beginning/end

The file format is JSON-lines, human-readable and editable.

## Testing Locally

```bash
# From the docs/ directory
cd docs

# Start a simple HTTP server
python3 -m http.server 8000

# Or with Node.js
npx serve .

# Or with Rust
cargo install miniserve
miniserve .
```

Then open http://localhost:8000 in your browser.

**Note:** The `.cast` file must be served over HTTP (not `file://`) for the fetch to work.

## Deploying to GitHub Pages

### Option 1: Use `/docs` directory (already configured)

1. Push the `docs/` directory to your main branch
2. Go to repo Settings → Pages
3. Source: "Deploy from a branch"
4. Branch: `main`, Folder: `/docs`
5. Save

The page will be available at `https://<username>.github.io/agents-on-tmux/`

### Option 2: Custom domain

If you have a custom domain:
1. Add a `CNAME` file in `docs/` with your domain
2. Configure DNS records with your domain provider
3. Enable HTTPS in GitHub Pages settings

## Terminal Player Features

The embedded terminal player supports:
- **Play/Pause** — Control playback
- **Restart** — Replay from the beginning
- **Speed control** — 1x, 2x, 4x playback speed
- **Progress bar** — Visual indication of playback position
- **Graceful fallback** — Shows "Demo coming soon" if no `.cast` file exists

## Customization

### Colors

The terminal theme is configured in `assets/demo.js`:

```javascript
const THEME = {
  background: "#1a1b26",
  foreground: "#a9b1d6",
  // ... more colors
};
```

The page uses a dark GitHub-inspired palette defined in `index.html` with Tailwind config.

### Terminal size

The terminal is fixed at 120 columns × 38 rows (1024×768 pixels). To change:

1. Update `cols` and `rows` in `assets/demo.js`
2. Update `--cols` and `--rows` when recording
3. Adjust the CSS in `index.html` if needed

### Content

All sections are in `index.html`. Edit directly — it's plain HTML with Tailwind classes.

## Development

### Local changes

Edit `index.html` and refresh your browser. Changes are immediate (no build step).

### Tailwind CSS

The page uses Tailwind CSS via CDN for rapid development. All styles are in the HTML with utility classes.

### xterm.js

The terminal emulator is loaded from CDN. The player logic is in `assets/demo.js`.

## Troubleshooting

**Terminal not showing?**
- Check that `demo.cast` exists in `assets/`
- Ensure you're serving over HTTP (not `file://`)
- Check browser console for errors

**Recording looks wrong?**
- Make sure to record with `--cols 120 --rows 38`
- The player expects asciinema .cast v2 format

**Styles not loading?**
- Tailwind and xterm.js are loaded from CDN
- Check your internet connection
- Verify CDN URLs in `index.html`

## Future enhancements

Potential improvements:
- [ ] Multiple demo recordings (different workflows)
- [ ] Interactive mode (let users type commands)
- [ ] Keyboard shortcuts for player controls
- [ ] Loop toggle
- [ ] Fullscreen mode
- [ ] Dark/light theme toggle
- [ ] Analytics integration
