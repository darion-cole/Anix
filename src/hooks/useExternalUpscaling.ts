/**
 * macOS only: tells the backend the player's Anime4K upscaling choice, so that mpv opened from
 * Anix (Open in mpv, local files, playlists) applies it. macOS has no embedded mpv player, which
 * is where the shaders run on Linux. See `commands/external_upscaling.rs`.
 */
import { useEffect } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { usePlayerStore } from '../stores/playerStore';
import { isMacOS } from '../lib/platform';

export function useExternalUpscaling(): void {
  const anime4k = usePlayerStore((s) => s.anime4k);
  useEffect(() => {
    if (!isMacOS()) return;
    invoke('set_external_upscaling', { preset: anime4k }).catch(() => {}); // i18n-ignore
  }, [anime4k]);
}
