/** When the update dialog opens by itself after the launch check. No imports, so node test scripts can load it. */

export interface UpdatePromptState {
  /** The version the update check found, or null when Anix is up to date. */
  available: string | null;
  /** That version was already offered in this run (the user may have chosen Later). */
  alreadyPrompted: boolean;
  /** An episode is playing on the Watch screen; a dialog would cover it. */
  onWatchScreen: boolean;
  /** Another dialog is open (setup, sign-in, the update dialog itself…). */
  dialogOpen: boolean;
}

/**
 * Whether to open the update dialog now: a newer version was found, it has not been offered in
 * this run yet, and nothing would be interrupted. When it is `false` only because of the Watch
 * screen or another dialog, asking again later can still give `true`.
 */
export function shouldPromptForUpdate(state: UpdatePromptState): boolean {
  return Boolean(state.available) && !state.alreadyPrompted && !state.onWatchScreen && !state.dialogOpen;
}
