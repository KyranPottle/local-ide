import { createContext, useContext } from "solid-js"
import type { RevertResult } from "../agent/toolHost"

export type TranscriptActions = {
  readonly modelLabel: (id: string) => string
  readonly openFile: (path: string, line?: number) => void
  readonly editAndResend: (seq: number, text: string) => void
  readonly forkThrough: (seq: number) => void
  readonly revert: (callId: string) => Promise<RevertResult>
  readonly approve: (callId: string) => void
  readonly decline: (callId: string) => void
  readonly continueRun: () => void
}

export const TranscriptActionsContext = createContext<TranscriptActions>()

export function useTranscriptActions(): TranscriptActions {
  const actions = useContext(TranscriptActionsContext)
  if (!actions) throw new Error("a transcript renders inside TranscriptActionsContext")
  return actions
}
