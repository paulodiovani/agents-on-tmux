/**
 * Asciinema .cast v2 player for xterm.js
 *
 * Reads a .cast file (JSON-lines format) and plays it back in an xterm.js
 * terminal with timing, play/pause, restart, and speed controls.
 *
 * If the .cast file is unavailable, displays a "coming soon" placeholder.
 */

(function () {
  "use strict";

  // ---------------------------------------------------------------------------
  // Configuration
  // ---------------------------------------------------------------------------

  const CAST_URL = "assets/demo.cast";
  const DEFAULT_SPEED = 1;
  const SPEEDS = [1, 2, 4];

  // Terminal theme — dark, neutral palette that matches the page.
  const THEME = {
    background: "#1a1b26",
    foreground: "#a9b1d6",
    cursor: "#c0caf5",
    cursorAccent: "#1a1b26",
    selectionBackground: "#33467c",
    selectionForeground: "#a9b1d6",
    black: "#15161e",
    red: "#f7768e",
    green: "#9ece6a",
    yellow: "#e0af68",
    blue: "#7aa2f7",
    magenta: "#bb9af7",
    cyan: "#7dcfff",
    white: "#a9b1d6",
    brightBlack: "#414868",
    brightRed: "#f7768e",
    brightGreen: "#9ece6a",
    brightYellow: "#e0af68",
    brightBlue: "#7aa2f7",
    brightMagenta: "#bb9af7",
    brightCyan: "#7dcfff",
    brightWhite: "#c0caf5",
  };

  // ---------------------------------------------------------------------------
  // DOM references
  // ---------------------------------------------------------------------------

  const termEl = document.getElementById("terminal");
  const playBtn = document.getElementById("btn-play");
  const restartBtn = document.getElementById("btn-restart");
  const speedBtn = document.getElementById("btn-speed");
  const statusEl = document.getElementById("player-status");
  const progressEl = document.getElementById("player-progress");
  const containerEl = document.getElementById("terminal-container");

  // ---------------------------------------------------------------------------
  // Terminal setup
  // ---------------------------------------------------------------------------

  // GlobalThis.Terminal is provided by the xterm.js CDN script loaded in HTML.
  /* global Terminal */
  const term = new Terminal({
    cols: 120,
    rows: 38,
    theme: THEME,
    fontFamily: "'JetBrains Mono', 'Fira Code', 'Cascadia Code', Menlo, monospace",
    fontSize: 13,
    lineHeight: 1.3,
    cursorBlink: false,
    cursorStyle: "block",
    disableStdin: true,
    scrollback: 0,
    convertEol: true,
  });

  term.open(termEl);
  showPlaceholder();

  // ---------------------------------------------------------------------------
  // Playback state
  // ---------------------------------------------------------------------------

  let events = []; // parsed .cast events [[delay, type, data], ...]
  let totalDuration = 0; // total recording duration in seconds
  let currentIndex = 0; // index of the next event to play
  let playing = false;
  let speed = DEFAULT_SPEED;
  let timerId = null;
  let playStartWall = 0; // wall-clock time when playback started/resumed
  let playStartOffset = 0; // recording-time offset when playback started/resumed

  // ---------------------------------------------------------------------------
  // Placeholder (shown when no .cast file exists yet)
  // ---------------------------------------------------------------------------

  function showPlaceholder() {
    term.clear();
    term.writeln("");
    term.writeln("  \x1b[90m┌─────────────────────────────────────────────────┐\x1b[0m");
    term.writeln("  \x1b[90m│                                                 │\x1b[0m");
    term.writeln("  \x1b[90m│\x1b[0m  \x1b[33m▐▛███▜▌\x1b[0m                                \x1b[90m│\x1b[0m");
    term.writeln("  \x1b[90m│\x1b[0m \x1b[33m▝▜█████▛▘\x1b[0m                                 \x1b[90m│\x1b[0m");
    term.writeln("  \x1b[90m│\x1b[0m   \x1b[33m▘▘ ▝▝\x1b[0m                                   \x1b[90m│\x1b[0m");
    term.writeln("  \x1b[90m│                                                 │\x1b[0m");
    term.writeln("  \x1b[90m│\x1b[0m  \x1b[1;37mAgents on TMUX\x1b[0m                             \x1b[90m│\x1b[0m");
    term.writeln("  \x1b[90m│                                                 │\x1b[0m");
    term.writeln("  \x1b[90m│\x1b[0m  \x1b[90mDemo coming soon...\x1b[0m                           \x1b[90m│\x1b[0m");
    term.writeln("  \x1b[90m│                                                 │\x1b[0m");
    term.writeln("  \x1b[90m└─────────────────────────────────────────────────┘\x1b[0m");
    term.writeln("");

    // Hide controls when there's no recording.
    if (containerEl) containerEl.classList.add("no-recording");
    setStatus("No recording yet");
  }

  // ---------------------------------------------------------------------------
  // .cast file loader
  // ---------------------------------------------------------------------------

  async function loadCast(url) {
    try {
      const response = await fetch(url);
      if (!response.ok) throw new Error("Not found: " + response.status);
      const text = await response.text();
      const lines = text.trim().split("\n");
      if (lines.length < 2) throw new Error("Empty recording");

      // First line is the header — we read width/height but don't enforce them.
      // const header = JSON.parse(lines[0]);

      events = [];
      for (let i = 1; i < lines.length; i++) {
        const line = lines[i].trim();
        if (!line) continue;
        try {
          const entry = JSON.parse(line);
          // entry: [delay, event_type, data]
          if (Array.isArray(entry) && entry.length >= 3) {
            events.push(entry);
          }
        } catch {
          // skip malformed lines
        }
      }

      if (events.length === 0) throw new Error("No events in recording");

      totalDuration = events[events.length - 1][0];
      return true;
    } catch (err) {
      console.warn("Failed to load cast file:", err.message);
      return false;
    }
  }

  // ---------------------------------------------------------------------------
  // Playback engine
  // ---------------------------------------------------------------------------

  function play() {
    if (events.length === 0) return;
    if (currentIndex >= events.length) {
      restart();
      return;
    }

    playing = true;
    playStartWall = performance.now();
    playStartOffset = events[currentIndex][0];
    updatePlayButton();
    setStatus("Playing");
    scheduleNext();
  }

  function pause() {
    playing = false;
    if (timerId !== null) {
      clearTimeout(timerId);
      timerId = null;
    }
    updatePlayButton();
    setStatus("Paused");
  }

  function restart() {
    if (timerId !== null) {
      clearTimeout(timerId);
      timerId = null;
    }
    playing = false;
    currentIndex = 0;
    term.reset();
    updateProgress(0);
    updatePlayButton();
    setStatus("Ready");
    // Auto-play after restart.
    play();
  }

  function scheduleNext() {
    if (!playing || currentIndex >= events.length) {
      if (currentIndex >= events.length) {
        playing = false;
        updatePlayButton();
        setStatus("Finished");
        updateProgress(1);
      }
      return;
    }

    const entry = events[currentIndex];
    const delay = entry[0];
    const eventType = entry[1];
    const data = entry[2];

    // Calculate how long to wait, accounting for speed and elapsed time.
    const recordingTimeSinceStart = delay - playStartOffset;
    const wallTimeToWait = (recordingTimeSinceStart / speed) * 1000;
    const elapsed = performance.now() - playStartWall;
    const wait = Math.max(0, wallTimeToWait - elapsed);

    timerId = setTimeout(() => {
      if (!playing) return;

      // Process the event.
      if (eventType === "o") {
        term.write(data);
      } else if (eventType === "r") {
        // Resize event: data is [cols, rows] — we ignore for fixed-size playback.
      }

      currentIndex++;
      updateProgress(delay / totalDuration);
      scheduleNext();
    }, wait);
  }

  // ---------------------------------------------------------------------------
  // Speed control
  // ---------------------------------------------------------------------------

  function cycleSpeed() {
    const idx = SPEEDS.indexOf(speed);
    speed = SPEEDS[(idx + 1) % SPEEDS.length];
    speedBtn.textContent = speed + "x";

    // If currently playing, restart timing from current position.
    if (playing && currentIndex < events.length) {
      playStartOffset = events[currentIndex][0];
      playStartWall = performance.now();
      if (timerId !== null) clearTimeout(timerId);
      scheduleNext();
    }
  }

  // ---------------------------------------------------------------------------
  // UI helpers
  // ---------------------------------------------------------------------------

  function updatePlayButton() {
    if (playing) {
      playBtn.innerHTML =
        '<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16" viewBox="0 0 24 24" fill="currentColor"><rect x="6" y="4" width="4" height="16"/><rect x="14" y="4" width="4" height="16"/></svg>';
      playBtn.title = "Pause";
    } else {
      playBtn.innerHTML =
        '<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16" viewBox="0 0 24 24" fill="currentColor"><polygon points="5,3 19,12 5,21"/></svg>';
      playBtn.title = "Play";
    }
  }

  function updateProgress(fraction) {
    if (progressEl) {
      progressEl.style.width = Math.min(100, fraction * 100) + "%";
    }
  }

  function setStatus(text) {
    if (statusEl) statusEl.textContent = text;
  }

  // ---------------------------------------------------------------------------
  // Event listeners
  // ---------------------------------------------------------------------------

  if (playBtn) {
    playBtn.addEventListener("click", () => {
      if (events.length === 0) return;
      if (playing) {
        pause();
      } else {
        if (currentIndex >= events.length) {
          restart();
        } else {
          play();
        }
      }
    });
  }

  if (restartBtn) {
    restartBtn.addEventListener("click", () => {
      if (events.length === 0) return;
      restart();
    });
  }

  if (speedBtn) {
    speedBtn.addEventListener("click", cycleSpeed);
  }

  // ---------------------------------------------------------------------------
  // Init — try to load the recording, fall back to placeholder.
  // ---------------------------------------------------------------------------

  (async () => {
    const loaded = await loadCast(CAST_URL);
    if (loaded) {
      if (containerEl) containerEl.classList.remove("no-recording");
      setStatus("Ready");
      // Auto-play when recording is available.
      play();
    }
  })();
})();
