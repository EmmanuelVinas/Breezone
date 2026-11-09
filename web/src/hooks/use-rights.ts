import { useEffect } from "react"
import { useQuery } from "@tanstack/react-query"
import { ALL_RIGHTS, api, type UserRights } from "@/lib/api"
import { setLang } from "@/lib/i18n"

/**
 * What the signed-in user may change besides driving. Everything until /api/me answers: the
 * server refuses what is not allowed anyway, this only hides the buttons.
 */
export function useRights(): UserRights {
  const me = useQuery({ queryKey: ["me"], queryFn: api.me, staleTime: 60_000 })
  return me.data?.rights ?? ALL_RIGHTS
}

/** Applies the language of the profile (reloads the page when it differs from the shown one). */
export function useProfileLanguage() {
  const me = useQuery({ queryKey: ["me"], queryFn: api.me, staleTime: 60_000 })
  const language = me.data?.language
  useEffect(() => {
    if (language !== undefined) setLang(language)
  }, [language])
}
