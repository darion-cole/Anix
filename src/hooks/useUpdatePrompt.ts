/**
 * Opens the update dialog by itself when the launch check finds a newer version, once per run
 * and per version. It waits while an episode plays or another dialog is open, and offers the
 * update as soon as that ends. Choosing Later in the dialog keeps it closed until the next start.
 */
import { useEffect } from 'react';
import { useLocation } from 'react-router-dom';
import { useUIStore } from '../stores/uiStore';
import { shouldPromptForUpdate } from '../lib/updatePrompt';

/** Versions already offered in this run. Module state, so it lasts until Anix quits. */
const promptedVersions = new Set<string>();

/** How often a waiting prompt looks again whether it may open. */
const RETRY_MS = 2000;

/** True when a modal dialog (any element with `role="dialog"`) is on screen. */
function anyDialogOpen(): boolean {
  return document.querySelector('[role="dialog"], [role="alertdialog"]') !== null;
}

export function useUpdatePrompt(availableVersion: string | null | undefined): void {
  const location = useLocation();
  const setUpdateDialogOpen = useUIStore((s) => s.setUpdateDialogOpen);
  const onWatchScreen = location.pathname.startsWith('/watch/');

  useEffect(() => {
    const available = availableVersion ?? null;
    if (!available || promptedVersions.has(available)) return;

    const tryOpen = (): boolean => {
      const open = shouldPromptForUpdate({
        available,
        alreadyPrompted: promptedVersions.has(available),
        onWatchScreen,
        dialogOpen: useUIStore.getState().updateDialogOpen || anyDialogOpen(),
      });
      if (open) {
        promptedVersions.add(available);
        setUpdateDialogOpen(true);
      }
      return open;
    };

    if (tryOpen() || onWatchScreen) return;
    // Another dialog is open (often the setup screen on a first start): look again shortly.
    const timer = window.setInterval(() => {
      if (tryOpen()) window.clearInterval(timer);
    }, RETRY_MS);
    return () => window.clearInterval(timer);
  }, [availableVersion, onWatchScreen, setUpdateDialogOpen]);
}
