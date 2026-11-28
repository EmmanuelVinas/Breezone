package com.breezone.app.ui.home

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import com.breezone.app.data.DayEnergy
import com.breezone.app.data.LinkyView
import com.breezone.app.resources.*
import com.breezone.app.ui.screens.Caption
import com.breezone.app.ui.screens.Muted
import com.breezone.app.ui.screens.SectionCard
import com.breezone.app.ui.screens.SectionTitle
import com.breezone.app.ui.screens.Stat
import com.breezone.app.ui.screens.SurplusCard
import com.breezone.app.ui.screens.formatEnergy
import com.breezone.app.ui.screens.rememberSurplus
import com.breezone.app.ui.theme.Fz
import kotlinx.coroutines.delay
import org.jetbrains.compose.resources.stringResource

/** What the home used, made, drew and returned today. */
@Composable
internal fun EnergyKpisBlock(ctx: HomeContext) {
    val t = ctx.day?.totals ?: return
    SectionCard {
        SectionTitle(stringResource(Res.string.block_energy_kpis))
        Row(Modifier.fillMaxWidth().padding(top = 8.dp)) {
            Stat(stringResource(Res.string.kpis_used), formatEnergy(t.consumption), modifier = Modifier.weight(1f))
            Stat(stringResource(Res.string.kpis_made), formatEnergy(t.production), Fz.colors.warning, Modifier.weight(1f))
        }
        Row(Modifier.fillMaxWidth().padding(top = 8.dp)) {
            Stat(stringResource(Res.string.kpis_drawn), formatEnergy(t.imported), modifier = Modifier.weight(1f))
            Stat(stringResource(Res.string.kpis_returned), formatEnergy(t.exported), Fz.colors.cool, Modifier.weight(1f))
        }
    }
}

/** A bar of [value] over [max], for a line of a list. */
@Composable
private fun Meter(value: Double, max: Double, color: Color, modifier: Modifier = Modifier) {
    Box(modifier.height(6.dp).background(MaterialTheme.colorScheme.surfaceVariant, RoundedCornerShape(6.dp))) {
        Box(Modifier.fillMaxWidth((value / max).toFloat().coerceIn(0f, 1f)).fillMaxHeight().background(color, RoundedCornerShape(6.dp)))
    }
}

/** What each measured device used today, the most first. */
@Composable
internal fun EnergyMeasuresBlock(ctx: HomeContext) {
    val consumers = ctx.day?.totals?.consumers.orEmpty().filter { (it.energy ?: 0.0) > 0 }.sortedByDescending { it.energy }
    if (consumers.isEmpty()) return
    val max = consumers.maxOf { it.energy ?: 0.0 }.coerceAtLeast(0.001)
    SectionCard {
        SectionTitle(stringResource(Res.string.block_energy_measures))
        consumers.forEach { c ->
            Row(Modifier.fillMaxWidth().padding(top = 10.dp), verticalAlignment = Alignment.CenterVertically) {
                Text(c.name, Modifier.width(120.dp), maxLines = 1, overflow = TextOverflow.Ellipsis)
                Meter(c.energy ?: 0.0, max, MaterialTheme.colorScheme.primary, Modifier.weight(1f).padding(horizontal = 10.dp))
                Text(formatEnergy(c.energy), fontWeight = FontWeight.Medium)
            }
        }
    }
}

/** The devices automations put on the solar surplus. */
@Composable
internal fun EnergySurplusBlock(ctx: HomeContext, editing: Boolean) {
    val surplus = rememberSurplus(ctx.hub, ctx.serverId)
    when {
        surplus != null && surplus.loads.isNotEmpty() -> SurplusCard(surplus)
        editing -> SectionCard { Muted(stringResource(Res.string.surplus_none)) }
    }
}

/** Bars of the last days, oldest first, with the value of the last one. */
@Composable
private fun DayBars(title: String, values: List<Pair<String, Double>>, color: Color, note: String? = null) {
    val max = values.maxOfOrNull { it.second }?.coerceAtLeast(0.001) ?: return
    SectionCard {
        SectionTitle(title)
        values.lastOrNull()?.let { (_, v) -> Text(formatEnergy(v), style = MaterialTheme.typography.titleLarge, modifier = Modifier.padding(top = 4.dp)) }
        if (note != null) Caption(note)
        Row(Modifier.fillMaxWidth().height(70.dp).padding(top = 8.dp), horizontalArrangement = Arrangement.spacedBy(5.dp), verticalAlignment = Alignment.Bottom) {
            values.forEach { (_, v) ->
                Box(Modifier.weight(1f).fillMaxHeight((v / max).toFloat().coerceIn(0.02f, 1f)).background(color, RoundedCornerShape(topStart = 5.dp, topEnd = 5.dp)))
            }
        }
        Row(Modifier.fillMaxWidth().padding(top = 4.dp)) {
            values.forEach { (day, _) -> Caption(day.takeLast(2), Modifier.weight(1f)) }
        }
    }
}

/** What the panels made each of the last 7 days. */
@Composable
internal fun SolarDaysBlock(ctx: HomeContext) {
    var days by remember(ctx.serverId) { mutableStateOf<List<DayEnergy>?>(null) }
    LaunchedEffect(ctx.serverId) {
        while (true) {
            runCatching { ctx.hub.api(ctx.serverId)?.energyDays(7) }.getOrNull()?.let { days = it }
            delay(30 * 60_000L)
        }
    }
    val list = days.orEmpty().map { it.day to (it.production ?: 0.0) }
    if (list.isEmpty()) return
    DayBars(stringResource(Res.string.block_solar_days), list, Fz.colors.warning)
}

/** What Enedis measured (Linky) on the last 7 days. */
@Composable
internal fun LinkyBlock(ctx: HomeContext, editing: Boolean) {
    var linky by remember(ctx.serverId) { mutableStateOf<LinkyView?>(null) }
    LaunchedEffect(ctx.serverId) {
        while (true) {
            runCatching { ctx.hub.api(ctx.serverId)?.linky(7) }.getOrNull()?.let { linky = it }
            delay(30 * 60_000L)
        }
    }
    val days = linky?.days.orEmpty().mapNotNull { d -> d.consumption?.let { d.day to it } }
    if (linky?.prm == null || days.isEmpty()) {
        if (editing) SectionCard { Muted(stringResource(Res.string.linky_none)) }
        return
    }
    DayBars(stringResource(Res.string.block_linky), days, Fz.colors.cool, stringResource(Res.string.linky_note))
}
