import type { HistoryPeriod } from "@/lib/periods"
import { PERIOD_LABEL } from "@/lib/periods"
import { t } from "@/lib/i18n"
import { ToggleGroup, ToggleGroupItem } from "@/components/ui/toggle-group"

/** 24 h, 7 j, 30 j, 1 an, 5 ans: only the periods the history covers (see `periodsFor`). */
export function PeriodPicker({
  value,
  periods,
  onChange,
}: {
  value: HistoryPeriod
  periods: HistoryPeriod[]
  onChange: (p: HistoryPeriod) => void
}) {
  if (periods.length < 2) return null
  return (
    <ToggleGroup
      type="single"
      variant="outline"
      size="sm"
      value={value}
      onValueChange={(v) => v && onChange(v as HistoryPeriod)}
      aria-label={t("Période")}
    >
      {periods.map((p) => (
        <ToggleGroupItem key={p} value={p}>
          {PERIOD_LABEL[p]}
        </ToggleGroupItem>
      ))}
    </ToggleGroup>
  )
}
