import { useEffect, useState } from "react"
import { useQueryClient } from "@tanstack/react-query"
import type { FeedEvent, Installation } from "@/lib/api"
import { getAccessToken } from "@/lib/api"

export type LiveState = "connecting" | "live" | "offline"

type Message =
  | { type: "hello" }
  | { type: "snapshot"; installations: Installation[] }
  | { type: "installation"; installation: Installation }
  | { type: "installation_removed"; id: number }
  | { type: "event"; event: FeedEvent }
  | { type: "routines_changed" }
  | { type: "error"; error: string }

/**
 * Keeps the React Query cache in sync with the server through the WebSocket push channel.
 * Reconnects automatically with backoff.
 */
export function useLive(enabled: boolean): LiveState {
  const qc = useQueryClient()
  const [state, setState] = useState<LiveState>("connecting")

  useEffect(() => {
    if (!enabled) return
    let ws: WebSocket | null = null
    let stopped = false
    let attempt = 0
    let timer: ReturnType<typeof setTimeout> | undefined

    const schedule = () => {
      if (stopped) return
      setState("offline")
      const delay = Math.min(30_000, 1000 * 2 ** attempt++)
      timer = setTimeout(connect, delay)
    }

    const connect = async () => {
      if (stopped) return
      setState("connecting")
      const token = await getAccessToken()
      if (!token || stopped) return schedule()
      const proto = location.protocol === "https:" ? "wss" : "ws"
      ws = new WebSocket(`${proto}://${location.host}/api/ws`)
      ws.onopen = () => ws?.send(JSON.stringify({ type: "auth", token }))
      ws.onmessage = (e) => {
        if (typeof e.data !== "string" || e.data === "pong") return
        const msg = JSON.parse(e.data) as Message
        switch (msg.type) {
          case "hello":
            attempt = 0
            setState("live")
            break
          case "snapshot":
            qc.setQueryData(["status"], msg.installations)
            break
          case "installation":
            qc.setQueryData<Installation[]>(["status"], (old) => {
              const list = old ?? []
              const exists = list.some((i) => i.id === msg.installation.id)
              const next = exists
                ? list.map((i) =>
                    i.id === msg.installation.id ? msg.installation : i
                  )
                : [...list, msg.installation]
              return next.sort((a, b) => a.id - b.id)
            })
            break
          case "installation_removed":
            qc.setQueryData<Installation[]>(["status"], (old) =>
              (old ?? []).filter((i) => i.id !== msg.id)
            )
            break
          case "event":
            qc.setQueryData<FeedEvent[]>(["events"], (old) =>
              [msg.event, ...(old ?? [])].slice(0, 300)
            )
            if (msg.event.kind === "system")
              qc.invalidateQueries({ queryKey: ["cloud"] })
            break
          case "routines_changed":
            qc.invalidateQueries({ queryKey: ["routines"] })
            break
        }
      }
      ws.onclose = () => {
        ws = null
        schedule()
      }
      ws.onerror = () => ws?.close()
    }

    connect()
    return () => {
      stopped = true
      if (timer) clearTimeout(timer)
      ws?.close()
    }
  }, [enabled, qc])

  return state
}
