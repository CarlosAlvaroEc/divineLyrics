import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import {
  IPC_COMMANDS,
  IPC_EVENTS,
  type InitialStateTuple,
  type LyricLine,
  type LyricsMode,
  type LyricsPayload,
  type PlaybackTick,
  type TrackInfo,
} from "./types/ipc";

/**
 * Estado local puramente reactivo (sin lógica de negocio ni llamadas HTTP).
 * Solo interpola el reloj entre pulsos del backend y proyecta el DOM
 * según el modo de Instagram Stories activo (`editorial`, `typewriter`, `classic`, `single`).
 */
interface OverlayViewState {
  track: TrackInfo | null;
  lyrics: LyricsPayload | null;
  isPlaying: boolean;
  basePositionMs: number;
  lastTickPerfTime: number;
  activeLineIndex: number;
  lastTypewriterText: string;
  locked: boolean;
  mode: LyricsMode;
}

const VALID_MODES: readonly LyricsMode[] = [
  "editorial",
  "typewriter",
  "classic",
  "single",
];

const state: OverlayViewState = {
  track: null,
  lyrics: null,
  isPlaying: false,
  basePositionMs: 0,
  lastTickPerfTime: performance.now(),
  activeLineIndex: -1,
  lastTypewriterText: "",
  locked: false,
  mode: "classic",
};

const overlayRoot = document.getElementById("overlay-root") as HTMLElement;
const lyricsStageEl = document.getElementById("lyrics-stage") as HTMLElement;
const lockPillBtn = document.getElementById("lock-pill-btn") as HTMLButtonElement;
const modeButtons = Array.from(
  document.querySelectorAll<HTMLButtonElement>("[data-mode-btn]")
);

function normalizeMode(raw: string): LyricsMode {
  return VALID_MODES.includes(raw as LyricsMode)
    ? (raw as LyricsMode)
    : "classic";
}

function getInterpolatedPositionMs(nowPerf: number): number {
  if (!state.isPlaying) {
    return state.basePositionMs;
  }
  const elapsed = Math.max(0, nowPerf - state.lastTickPerfTime);
  const interpolated = state.basePositionMs + elapsed;
  if (state.track && state.track.durationMs > 0) {
    return Math.min(interpolated, state.track.durationMs);
  }
  return interpolated;
}

function createInstrumentalDotsElement(): HTMLElement {
  const wrapper = document.createElement("span");
  wrapper.className = "instrumental-dots";
  wrapper.setAttribute("aria-label", "Pausa instrumental");
  for (let i = 0; i < 3; i++) {
    const dot = document.createElement("span");
    dot.className = "instrumental-dot";
    dot.textContent = "•";
    wrapper.appendChild(dot);
  }
  return wrapper;
}

function renderPlaceholder(message: string): void {
  const p = document.createElement("p");
  p.className = "lyric-placeholder";
  p.textContent = message;
  lyricsStageEl.replaceChildren(p);
}

function renderTrackMeta(track: TrackInfo | null): void {
  state.track = track;
  if (!track) {
    state.lyrics = null;
    state.activeLineIndex = -1;
    state.lastTypewriterText = "";
    renderPlaceholder("Abre Spotify en tu sistema para mostrar la letra aquí.");
    return;
  }

  if (!state.lyrics || state.lyrics.trackId !== track.id) {
    state.lyrics = null;
    state.activeLineIndex = -1;
    state.lastTypewriterText = "";
    renderPlaceholder("Buscando letra en LRCLIB…");
  }
}

function renderLyrics(payload: LyricsPayload): void {
  if (state.track && payload.trackId !== state.track.id) {
    return;
  }
  state.lyrics = payload;
  state.activeLineIndex = -1;
  state.lastTypewriterText = "";

  if (payload.lines.length === 0) {
    renderPlaceholder("Letra no disponible para esta pista.");
    return;
  }

  if (!payload.synced) {
    renderPlainLyrics(payload.lines);
    return;
  }

  updateActiveView(getInterpolatedPositionMs(performance.now()), true);
}

function renderPlainLyrics(lines: LyricLine[]): void {
  const container = document.createElement("div");
  container.className = "lyrics-plain-scroll";
  for (const line of lines) {
    const p = document.createElement("p");
    p.className = "ig-lyric-text";
    p.textContent = line.text || "♪";
    container.appendChild(p);
  }
  lyricsStageEl.replaceChildren(container);
}

function applyPlaybackTick(tick: PlaybackTick): void {
  state.basePositionMs = tick.positionMs;
  state.isPlaying = tick.isPlaying;
  state.lastTickPerfTime = performance.now();
  updateActiveView(tick.positionMs, false);
}

function applyInteractionMode(locked: boolean): void {
  state.locked = locked;
  overlayRoot.dataset.locked = String(locked);
  overlayRoot.classList.toggle("is-unlocked", !locked);
}

function applyLyricsMode(mode: LyricsMode): void {
  state.mode = mode;
  overlayRoot.dataset.mode = mode;
  for (const btn of modeButtons) {
    btn.classList.toggle("is-selected", btn.dataset.modeBtn === mode);
  }

  state.lastTypewriterText = "";
  if (state.lyrics && state.lyrics.synced && state.lyrics.lines.length > 0) {
    updateActiveView(getInterpolatedPositionMs(performance.now()), true);
  }
}

function findActiveLineIndex(positionMs: number): number {
  const lyrics = state.lyrics;
  if (!lyrics || !lyrics.synced || lyrics.lines.length === 0) {
    return -1;
  }

  let activeIdx = -1;
  for (let i = 0; i < lyrics.lines.length; i++) {
    if (lyrics.lines[i].timeMs <= positionMs) {
      activeIdx = i;
    } else {
      break;
    }
  }
  return activeIdx;
}

function updateActiveView(positionMs: number, forceRender: boolean): void {
  if (!state.lyrics || !state.lyrics.synced || state.lyrics.lines.length === 0) {
    return;
  }

  const lines = state.lyrics.lines;
  const nextIndex = findActiveLineIndex(positionMs);
  const indexChanged = nextIndex !== state.activeLineIndex;

  if (state.mode === "typewriter") {
    renderTypewriterMode(lines, nextIndex, positionMs, forceRender || indexChanged);
    state.activeLineIndex = nextIndex;
    return;
  }

  if (!indexChanged && !forceRender) {
    return;
  }

  const previousIndex = state.activeLineIndex;
  state.activeLineIndex = nextIndex;

  switch (state.mode) {
    case "editorial":
      renderEditorialMode(lines, nextIndex);
      break;
    case "classic":
      renderClassicMode(lines, nextIndex);
      break;
    case "single":
      renderSingleMode(lines, nextIndex, previousIndex);
      break;
  }
}

/**
 * Modo 1: Editorial — Bloques de 3 a 4 líneas en mayúsculas alternando
 * tamaño por longitud (1.65rem para líneas cortas/clave, 0.95rem para conectores).
 */
function renderEditorialMode(lines: LyricLine[], activeIdx: number): void {
  const effectiveIdx = Math.max(0, activeIdx);
  const blockSize = 4;
  const blockStart = Math.floor(effectiveIdx / blockSize) * blockSize;
  const blockEnd = Math.min(lines.length, blockStart + blockSize);

  const fragment = document.createDocumentFragment();
  for (let i = blockStart; i < blockEnd; i++) {
    const lineText = lines[i].text.trim();
    const p = document.createElement("p");
    const sizeClass = lineText.length <= 22 ? "is-short" : "is-connector";
    const stateClass =
      activeIdx < 0 || i > activeIdx
        ? "is-future"
        : i === activeIdx
          ? "is-active"
          : "is-past";

    p.className = `ig-lyric-text editorial-line ${sizeClass} ${stateClass}`;
    p.textContent = lineText || "♪";
    fragment.appendChild(p);
  }

  lyricsStageEl.replaceChildren(fragment);
}

/**
 * Modo 2: Typewriter — Acumula hasta 4 líneas de la estrofa actual y revela
 * progresivamente los caracteres de la línea activa con cursor sólido (█).
 */
function renderTypewriterMode(
  lines: LyricLine[],
  activeIdx: number,
  positionMs: number,
  forceRender: boolean
): void {
  const effectiveIdx = Math.max(0, activeIdx);
  const blockSize = 4;
  const blockStart = Math.floor(effectiveIdx / blockSize) * blockSize;

  const activeLine = lines[effectiveIdx];
  const fullChars = Array.from(activeLine.text || "♪");

  let revealedText = "";
  if (activeIdx >= 0) {
    const startMs = activeLine.timeMs;
    const nextLine = lines[effectiveIdx + 1];
    const rawDuration = nextLine
      ? Math.max(100, nextLine.timeMs - startMs)
      : 3000;

    // Comprimir ventana de escritura al 65% (máx 2400ms), respetando 25ms/carácter sin exceder 90%
    const baseTypingDuration = Math.min(rawDuration * 0.65, 2400);
    const typingDuration = Math.max(
      1,
      Math.min(
        rawDuration * 0.9,
        Math.max(baseTypingDuration, fullChars.length * 25)
      )
    );

    const progress = Math.min(
      1,
      Math.max(0, (positionMs - startMs) / typingDuration)
    );
    const charCount = Math.max(1, Math.floor(progress * fullChars.length));
    revealedText = fullChars.slice(0, charCount).join("");
  }

  if (!forceRender && revealedText === state.lastTypewriterText) {
    return;
  }
  state.lastTypewriterText = revealedText;

  // Si seguimos en la misma línea, actualizar solo el texto revelado para no reiniciar el parpadeo del cursor
  if (!forceRender && activeIdx === state.activeLineIndex) {
    const activeSpan = lyricsStageEl.querySelector(".typewriter-revealed");
    if (activeSpan) {
      activeSpan.textContent = revealedText;
      return;
    }
  }

  const fragment = document.createDocumentFragment();
  for (let i = blockStart; i <= effectiveIdx; i++) {
    const p = document.createElement("p");
    p.className = "ig-lyric-text typewriter-line";

    if (i < effectiveIdx) {
      p.textContent = lines[i].text || "♪";
    } else {
      const spanText = document.createElement("span");
      spanText.className = "typewriter-revealed";
      spanText.textContent = revealedText;
      p.appendChild(spanText);

      const cursor = document.createElement("span");
      cursor.className = "typewriter-cursor";
      cursor.textContent = "█";
      p.appendChild(cursor);
    }

    fragment.appendChild(p);
  }

  lyricsStageEl.replaceChildren(fragment);
}

/**
 * Modo 3: Classic — Teleprompter centrado de exactamente 3 líneas:
 * [anterior, actual, siguiente].
 */
function renderClassicMode(lines: LyricLine[], activeIdx: number): void {
  const slots: Array<{ line: LyricLine | null; slotClass: string }> = [
    {
      line: activeIdx > 0 ? lines[activeIdx - 1] : null,
      slotClass: "slot-prev",
    },
    {
      line: activeIdx >= 0 ? lines[activeIdx] : lines[0],
      slotClass: "slot-active",
    },
    {
      line:
        activeIdx >= 0 && activeIdx + 1 < lines.length
          ? lines[activeIdx + 1]
          : activeIdx < 0 && lines.length > 1
            ? lines[1]
            : null,
      slotClass: "slot-next",
    },
  ];

  const fragment = document.createDocumentFragment();
  for (const { line, slotClass } of slots) {
    const p = document.createElement("p");
    p.className = `ig-lyric-text classic-line ${slotClass}`;

    if (!line) {
      p.classList.add("is-empty");
      p.textContent = "\u00A0";
    } else if (line.text.trim() === "" && slotClass === "slot-active") {
      p.appendChild(createInstrumentalDotsElement());
    } else {
      p.textContent = line.text || "\u00A0";
    }

    fragment.appendChild(p);
  }

  lyricsStageEl.replaceChildren(fragment);
}

/**
 * Modo 4: Single — Frase única en cursiva con transición de entrada/salida (200ms).
 */
function renderSingleMode(
  lines: LyricLine[],
  activeIdx: number,
  previousIdx: number
): void {
  const currentLine = activeIdx >= 0 ? lines[activeIdx] : lines[0];
  const outgoingEl = lyricsStageEl.querySelector(".single-line.is-entering");

  const incomingEl = document.createElement("p");
  incomingEl.className = "ig-lyric-text single-line is-entering";

  if (currentLine.text.trim() === "") {
    incomingEl.appendChild(createInstrumentalDotsElement());
  } else {
    incomingEl.textContent = currentLine.text;
  }

  if (outgoingEl && previousIdx !== activeIdx) {
    outgoingEl.classList.remove("is-entering");
    outgoingEl.classList.add("is-leaving");
    lyricsStageEl.appendChild(incomingEl);

    window.setTimeout(() => {
      outgoingEl.remove();
    }, 200);
  } else {
    lyricsStageEl.replaceChildren(incomingEl);
  }
}

function startInterpolationLoop(): void {
  const tickFrame = (nowPerf: number) => {
    if (state.track && state.isPlaying) {
      const currentPos = getInterpolatedPositionMs(nowPerf);
      updateActiveView(currentPos, false);
    }
    requestAnimationFrame(tickFrame);
  };
  requestAnimationFrame(tickFrame);
}

function bindUiControls(): void {
  applyInteractionMode(state.locked);
  applyLyricsMode(state.mode);

  // Permitir arrastrar la ventana desde el contenedor principal cuando no está bloqueada
  overlayRoot.addEventListener("mousedown", (event) => {
    if (state.locked || event.button !== 0) {
      return;
    }
    const target = event.target as HTMLElement;
    if (target.closest("button")) {
      return;
    }
    void getCurrentWindow().startDragging();
  });

  // Selector de los 4 modos de Instagram Stories + persistencia en settings.json
  for (const btn of modeButtons) {
    btn.addEventListener("click", async () => {
      const chosen = btn.dataset.modeBtn as LyricsMode | undefined;
      if (!chosen) {
        return;
      }
      applyLyricsMode(chosen);
      try {
        const persisted = await invoke<string>(
          IPC_COMMANDS.SAVE_STYLE_PREFERENCE,
          { style: chosen }
        );
        applyLyricsMode(normalizeMode(persisted));
      } catch (err) {
        console.error("Failed to save lyrics mode preference:", err);
      }
    });
  }

  // Pestaña flotante de bloqueo/desbloqueo en esquina superior izquierda
  lockPillBtn.addEventListener("mousedown", (event) => {
    event.stopPropagation();
  });

  lockPillBtn.addEventListener("click", async (event) => {
    event.stopPropagation();
    try {
      await invoke(IPC_COMMANDS.TOGGLE_CLICK_THROUGH, {
        locked: !state.locked,
      });
    } catch (err) {
      console.error("Failed to toggle overlay lock:", err);
    }
  });
}

async function bootstrap(): Promise<void> {
  bindUiControls();
  startInterpolationLoop();

  await listen<TrackInfo | null>(IPC_EVENTS.TRACK_CHANGED, (event) => {
    renderTrackMeta(event.payload);
  });

  await listen<LyricsPayload>(IPC_EVENTS.LYRICS_LOADED, (event) => {
    renderLyrics(event.payload);
  });

  await listen<PlaybackTick>(IPC_EVENTS.PLAYBACK_TICK, (event) => {
    applyPlaybackTick(event.payload);
  });

  await listen<boolean>(IPC_EVENTS.INTERACTION_MODE, (event) => {
    applyInteractionMode(event.payload);
  });

  try {
    const savedMode = await invoke<string>(IPC_COMMANDS.GET_STYLE_PREFERENCE);
    applyLyricsMode(normalizeMode(savedMode));
  } catch (err) {
    console.error("Failed to fetch lyrics mode preference:", err);
  }

  try {
    const initialState = await invoke<InitialStateTuple>(
      IPC_COMMANDS.GET_INITIAL_STATE
    );
    if (initialState) {
      const [track, lyrics, playback] = initialState;
      renderTrackMeta(track);
      if (lyrics) {
        renderLyrics(lyrics);
      }
      applyPlaybackTick(playback);
    }
  } catch (err) {
    console.error("Failed to fetch initial state from Tauri backend:", err);
  }
}

void bootstrap();
