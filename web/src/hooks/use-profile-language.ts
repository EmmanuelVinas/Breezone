import { useEffect } from "react"
import { useQuery } from "@tanstack/react-query"
import { api } from "@/lib/api"
import { setLang } from "@/lib/i18n"

/** Applies the language of the profile (reloads the page when it differs from the shown one). */
export function useProfileLanguage() {
  const me = useQuery({ queryKey: ["me"], queryFn: api.me, staleTime: 60_000 })
  const language = me.data?.language
  useEffect(() => {
    if (language !== undefined) setLang(language)
  }, [language])
}
