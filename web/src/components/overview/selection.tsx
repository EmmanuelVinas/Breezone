import { createContext, useContext } from "react"

/**
 * The device chosen on a page that shows its detail beside its blocks (lights, shutters,
 * ventilation): the blocks of the page mark it and choose it; elsewhere nothing is chosen.
 */
export type PageSelection = {
  selected: number | null
  select: (id: number) => void
}

export const SelectionContext = createContext<PageSelection | null>(null)

export function useSelection(): PageSelection {
  return useContext(SelectionContext) ?? { selected: null, select: () => {} }
}
