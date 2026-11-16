/**
 * Contratos de datos IPC sincronizados con `src-tauri/src/models.rs`
 * (`#[serde(rename_all = "camelCase")]`).
 */

export interface TrackInfo {
  /** Identificador normalizado "{artist}-{title}" en minúsculas para caché */
  id: string;
  title: string;
  /** Artista principal */
  artist: string;
  durationMs: number;
}

export interface LyricLine {
  /** Marca de tiempo en milisegundos; 0 si la letra no es sincronizada */
  timeMs: number;
  text: string;
}

export interface LyricsPayload {
  trackId: string;
  synced: boolean;
  lines: LyricLine[];
}

export interface PlaybackTick {
  positionMs: number;
  isPlaying: boolean;
}

export type InitialStateTuple = [
  TrackInfo,
  LyricsPayload | null,
  PlaybackTick,
] | null;

export const IPC_EVENTS = {
  TRACK_CHANGED: "spotify:track-changed",
  LYRICS_LOADED: "spotify:lyrics-loaded",
  PLAYBACK_TICK: "spotify:playback-tick",
  INTERACTION_MODE: "overlay:interaction-mode",
} as const;

export type LyricsMode = "editorial" | "typewriter" | "classic" | "single";

export const IPC_COMMANDS = {
  GET_INITIAL_STATE: "get_initial_state",
  TOGGLE_CLICK_THROUGH: "toggle_click_through",
  GET_STYLE_PREFERENCE: "get_style_preference",
  SAVE_STYLE_PREFERENCE: "save_style_preference",
} as const;
