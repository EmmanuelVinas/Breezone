package com.breezone.app.ui.screens

import androidx.compose.animation.core.LinearEasing
import androidx.compose.animation.core.RepeatMode
import androidx.compose.animation.core.animateFloat
import androidx.compose.animation.core.infiniteRepeatable
import androidx.compose.animation.core.rememberInfiniteTransition
import androidx.compose.animation.core.tween
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.IntrinsicSize
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.Immutable
import androidx.compose.runtime.getValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.Path
import androidx.compose.ui.graphics.PathEffect
import androidx.compose.ui.graphics.StrokeCap
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.breezone.app.data.EnergyLive
import com.breezone.app.resources.*
import com.breezone.app.ui.icons.FzIcons
import com.breezone.app.ui.theme.Fz
import kotlin.math.abs
import kotlin.math.max
import kotlin.math.roundToInt
import org.jetbrains.compose.resources.stringResource

/** A box of the diagram, and the link between it and the home. */
@Immutable
private data class FlowNode(
    val key: String,
    val icon: ImageVector,
    val label: String,
    val value: String,
    val sub: String?,
    val color: Color,
    /** The power on its link (W); below 5 W the link rests. */
    val power: Double?,
    /** The energy goes toward the home (else away from it). */
    val towardHome: Boolean,
)

/**
 * What the home draws and makes now, as the web shows it: the sources above (solar, battery,
 * grid), the home in the middle, what uses the power below; the dots of each link go the way the
 * energy goes.
 */
@Composable
internal fun EnergyNowCard(live: EnergyLive, titled: Boolean = true) {
    val roles = live.roles
    val sources = buildList {
        if (roles.solar) add(FlowNode("solar", FzIcons.Sun, stringResource(Res.string.energy_solar), formatPower(live.solar), null, Fz.colors.warning, live.solar, true))
        if (roles.battery) {
            val b = live.battery ?: 0.0
            val sub = stringResource(
                when {
                    abs(b) < 5 -> Res.string.energy_battery_idle
                    b > 0 -> Res.string.energy_battery_charging
                    else -> Res.string.energy_battery_discharging
                }
            )
            add(FlowNode("battery", FzIcons.BatteryMedium, stringResource(Res.string.energy_battery), formatPower(live.battery), sub, Fz.colors.success, live.battery, b <= 0))
        }
        if (roles.grid) {
            val g = live.grid ?: 0.0
            // Drawn from the grid is the usual case: only a power returned to it is said.
            val sub = if (g < 0) stringResource(Res.string.energy_grid_injected) else null
            add(FlowNode("grid", FzIcons.UtilityPole, stringResource(Res.string.energy_grid), formatPower(live.grid), sub, Fz.colors.cool, live.grid, g >= 0))
        }
    }
    val palette = listOf(Fz.colors.heat, MaterialTheme.colorScheme.primary, Fz.colors.fan, Fz.colors.air)
    val consumers = buildList {
        live.consumers.take(3).forEachIndexed { i, c ->
            add(FlowNode("c-${c.deviceId}-${c.roomId}-${c.name}", FzIcons.Plug, c.name, formatPower(c.power), null, palette[i % palette.size], c.power, false))
        }
        if (live.other != null && live.consumers.isNotEmpty()) {
            add(FlowNode("other", FzIcons.Plug, stringResource(Res.string.energy_other), formatPower(live.other), null, Fz.colors.muted, live.other, false))
        }
    }
    val home = live.home
    val covered = if (home != null && home > 0 && live.grid != null && (roles.solar || roles.battery)) {
        (((home - max(0.0, live.grid)) / home).coerceIn(0.0, 1.0) * 100).roundToInt()
    } else {
        null
    }
    val surplus = live.grid?.takeIf { it < 0 }?.let { -it }

    val transition = rememberInfiniteTransition()
    // From 0 to 1 again and again: how far the dots of the links have moved.
    val phase by transition.animateFloat(0f, 1f, infiniteRepeatable(tween(900, easing = LinearEasing), RepeatMode.Restart))

    SectionCard {
        if (titled) {
            Row(verticalAlignment = Alignment.CenterVertically) {
                Column(Modifier.weight(1f)) {
                    SectionTitle(stringResource(Res.string.energy_now))
                    Caption(stringResource(Res.string.energy_flow_hint))
                }
                if (surplus != null) SurplusBadge(formatPower(surplus))
            }
        }
        if (sources.isNotEmpty()) {
            NodeRow(sources, keepColumns = false)
            Links(sources, phase = { phase }, fromTop = true)
        }
        HomeCircle(formatPower(home), covered)
        if (consumers.isNotEmpty()) {
            Links(consumers.take(2), phase = { phase }, fromTop = false)
            consumers.chunked(2).forEachIndexed { i, row -> NodeRow(row, keepColumns = i > 0) }
        }
    }
}

@Composable
private fun SurplusBadge(power: String) {
    Row(
        Modifier.clip(CircleShape).border(1.dp, Fz.colors.muted.copy(alpha = 0.4f), CircleShape).padding(horizontal = 10.dp, vertical = 4.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Box(Modifier.size(8.dp).clip(CircleShape).background(Fz.colors.success))
        Spacer(Modifier.width(6.dp))
        Text(stringResource(Res.string.energy_surplus, power), fontSize = 12.sp)
    }
}

/** A row of boxes; `keepColumns`: a lone box keeps the width (and the column) of one of two. */
@Composable
private fun NodeRow(nodes: List<FlowNode>, keepColumns: Boolean) {
    // The boxes of a row take the height of the tallest one.
    Row(Modifier.fillMaxWidth().height(IntrinsicSize.Min), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
        nodes.forEach { NodeBox(it, Modifier.weight(1f).fillMaxHeight()) }
        if (keepColumns && nodes.size == 1) Spacer(Modifier.weight(1f))
    }
}

@Composable
private fun NodeBox(node: FlowNode, modifier: Modifier) {
    val shape = RoundedCornerShape(16.dp)
    Column(
        modifier
            .clip(shape)
            .background(node.color.copy(alpha = 0.10f))
            .border(1.dp, node.color.copy(alpha = 0.35f), shape)
            .padding(horizontal = 12.dp, vertical = 10.dp),
        verticalArrangement = Arrangement.spacedBy(2.dp),
    ) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            Icon(node.icon, null, Modifier.size(14.dp), tint = node.color)
            Spacer(Modifier.width(6.dp))
            Text(node.label, fontSize = 12.sp, fontWeight = FontWeight.Medium, color = node.color, maxLines = 1, overflow = TextOverflow.Ellipsis)
        }
        Text(node.value, fontSize = 20.sp, fontWeight = FontWeight.SemiBold, maxLines = 1)
        if (node.sub != null) Text(node.sub, fontSize = 12.sp, color = Fz.colors.muted, maxLines = 1, overflow = TextOverflow.Ellipsis)
    }
}

/** The home: what it uses now, and the share of it the solar (and battery) cover. */
@Composable
private fun HomeCircle(power: String, covered: Int?) {
    Box(Modifier.fillMaxWidth(), contentAlignment = Alignment.Center) {
        Box(
            Modifier.size(136.dp).clip(CircleShape).border(8.dp, Fz.colors.muted.copy(alpha = 0.18f), CircleShape),
            contentAlignment = Alignment.Center,
        ) {
            Column(horizontalAlignment = Alignment.CenterHorizontally) {
                Text(stringResource(Res.string.energy_home), fontSize = 12.sp, color = Fz.colors.muted)
                Text(power, fontSize = 24.sp, fontWeight = FontWeight.SemiBold, textAlign = TextAlign.Center)
                if (covered != null) Text(stringResource(Res.string.energy_covered, covered), fontSize = 12.sp, color = MaterialTheme.colorScheme.primary)
            }
        }
    }
}

/**
 * The links between a row of boxes and the home: from the middle of each box's column to the
 * middle of the card, dotted, the dots moving the way the energy goes.
 */
@Composable
private fun Links(nodes: List<FlowNode>, phase: () -> Float, fromTop: Boolean) {
    Canvas(Modifier.fillMaxWidth().height(36.dp)) {
        val n = nodes.size
        val dot = 2.dp.toPx()
        val gap = 8.dp.toPx()
        val period = dot + gap
        val center = Offset(size.width / 2, if (fromTop) size.height else 0f)
        nodes.forEachIndexed { i, node ->
            val column = Offset(size.width * (i + 0.5f) / n, if (fromTop) 0f else size.height)
            // Drawn in the way the energy goes, so that the dots move along with it.
            val toHome = if (fromTop) node.towardHome else !node.towardHome
            val (from, to) = if (toHome == fromTop) column to center else center to column
            val idle = node.power == null || abs(node.power) < 5
            val path = Path().apply {
                moveTo(from.x, from.y)
                lineTo(to.x, to.y)
            }
            drawPath(
                path,
                color = node.color.copy(alpha = if (idle) 0.25f else 0.9f),
                style = Stroke(
                    width = dot,
                    cap = StrokeCap.Round,
                    pathEffect = PathEffect.dashPathEffect(floatArrayOf(0.01f, period), if (idle) 0f else -phase() * period),
                ),
            )
        }
    }
}
