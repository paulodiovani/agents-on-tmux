# AOT Landing Page

This directory contains the static landing page for Agents on TMUX, hosted on GitHub Pages.

## Structure

- `index.html` — Main landing page with demo screenshot
- `assets/demo.png` — Demo screenshot of AOT in action
- `user-manual/` — Documentation pages
  - `recommended-tmux-config.md` — Source markdown
  - `recommended-tmux-config.html` — Rendered HTML page
- `README.md` — This file

## Demo

A live recording is available at [asciinema.org/a/1268923](https://asciinema.org/a/1268923).

The landing page also displays a static screenshot at `assets/demo.png`.

To update the screenshot:
1. Run AOT in your terminal with a realistic setup (multiple agent windows, TUI panel visible, icons enabled)
2. Take a screenshot and save it as `assets/demo.png`

**Recommended specifications:**
- **Size**: 1024x768 or similar wide format
- **Content**: Show AOT with multiple agent windows, the TUI control panel, and icons visible
- **Format**: PNG for best quality

## Customization

### Screenshot

Update the screenshot by replacing `assets/screenshot.png`. The image container is set to max-width: 1024px and will scale responsively.

### Colors

The page uses a dark GitHub-inspired palette defined in `index.html` with Tailwind config:

```html
<script>
  tailwind.config = {
    theme: {
      extend: {
        colors: {
          // Primary colors
          bg: '#0d1117',
          surface: '#161b22',
          border: '#30363d',
          text: '#c9d1d9',
          muted: '#8b949e',
          // Accent color (Rust orange)
          accent: '#f74c00',
        }
      }
    }
  }
</script>
```

### Content

All sections are in `index.html`. Edit directly — it's plain HTML with Tailwind classes.

## Deployment

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

## Development

### Local testing

```bash
cd docs
python3 -m http.server 8000
# Open http://localhost:8000
```

### Live editing

Edit `index.html` and refresh your browser. Changes are immediate (no build step).

### Tailwind CSS

The page uses Tailwind CSS via CDN for rapid development. All styles are in the HTML with utility classes.

## Troubleshooting

**Screenshot not showing?**
- Check that `assets/screenshot.png` exists
- Verify the file path is correct in `index.html`
- Check browser console for errors

**Styles not loading?**
- Tailwind is loaded from CDN
- Check your internet connection
- Verify CDN URL in `index.html`

**Image looks pixelated?**
- Use a higher resolution screenshot
- Ensure the PNG is not compressed too aggressively
- Consider using a 2x resolution image for retina displays

## Future enhancements

Potential improvements:
- [ ] Add a demo video or animated GIF
- [ ] Interactive terminal demo (using xterm.js or asciinema)
- [ ] Multi-language support
- [ ] Dark/light theme toggle
- [ ] Analytics integration
- [ ] Blog or changelog section
- [ ] Social sharing meta tags
