package com.breezone.app.ui.components

import androidx.compose.animation.animateColorAsState
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Switch
import androidx.compose.material3.SwitchDefaults
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.Stable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.runtime.setValue
import androidx.compose.runtime.snapshotFlow
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.drawWithCache
import androidx.compose.ui.geometry.CornerRadius
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.breezone.app.data.CloudInstallation
import com.breezone.app.data.Installation
import com.breezone.app.data.Zone
import com.breezone.app.resources.*
import com.breezone.app.ui.clampSetpoint
import com.breezone.app.ui.formatTemp
import com.breezone.app.ui.hueOf
import com.breezone.app.ui.installationPlace
import com.breezone.app.ui.icon
import com.breezone.app.ui.icons.FzIcons
import com.breezone.app.ui.label
import com.breezone.app.ui.theme.Fz
import com.breezone.app.ui.theme.TempStyle
import com.breezone.app.ui.tone
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.collectLatest
import org.jetbrains.compose.resources.pluralStringResource
import org.jetbrains.compose.resources.stringResource

fun hsl(h: Float, s: Float, l: Float) = Color.hsl(h % 360f, s, l)

private val CardShape = RoundedCornerShape(26.dp)

@Composable
fun StatusDot(online: Boolean, unknown: Boolean = false, modifier: Modifier = Modifier) {
    val color = when {
        unknown -> Fz.colors.muted
        online -> Fz.colors.success
        else -> MaterialTheme.colorScheme.error
    }
    Box(modifier.size(8.dp).clip(CircleShape).background(color))
}

@Composable
fun connectionLabel(state: String): String = stringResource(
    when (state) {
        "online" -> Res.string.connection_online
        "offline" -> Res.string.connection_offline
        else -> Res.string.connection_unknown
    },
)

/** Installation card, like the colored cards of the official app's "Mes installations". */
@Composable
fun InstallationCard(inst: Installation, serverLabel: String?, onClick: () -> Unit) {
    val hue = hueOf(inst.name)
    val on = inst.zones.count { it.on }
    val temps = inst.zones.mapNotNull { it.roomTemp }
    val zonesOn = pluralStringResource(Res.plurals.installation_zones_on, on, on, inst.zones.size)
    Box(
        Modifier
            .fillMaxWidth()
            .height(150.dp)
            .clip(RoundedCornerShape(28.dp))
            .background(Brush.linearGradient(listOf(hsl(hue, 0.55f, 0.42f), hsl(hue + 40f, 0.5f, 0.26f))))
            .clickable(onClick = onClick)
            .padding(18.dp),
    ) {
        Icon(
            FzIcons.House,
            null,
            tint = Color.White.copy(alpha = 0.12f),
            modifier = Modifier.size(110.dp).align(Alignment.BottomEnd).padding(start = 8.dp),
        )
        Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.SpaceBetween, verticalAlignment = Alignment.CenterVertically) {
            Pill(
                text = connectionLabel(inst.connection.state),
                leading = { StatusDot(inst.connection.online, inst.connection.state == "unknown") },
            )
            if (temps.isNotEmpty()) {
                Text(formatTemp(temps.average()), color = Color.White, style = TempStyle, fontSize = 16.sp)
            }
        }
        Column(Modifier.align(Alignment.BottomStart)) {
            Text(inst.name, color = Color.White, style = MaterialTheme.typography.titleLarge, maxLines = 1, overflow = TextOverflow.Ellipsis)
            Text(
                installationPlace(inst),
                color = Color.White.copy(alpha = 0.85f),
                style = MaterialTheme.typography.bodySmall,
                maxLines = 1,
            )
            Text(
                if (serverLabel != null) "$zonesOn · $serverLabel" else zonesOn,
                color = Color.White.copy(alpha = 0.85f),
                style = MaterialTheme.typography.bodySmall,
                maxLines = 1,
            )
        }
    }
}

@Composable
fun RemoteInstallationCard(inst: CloudInstallation, onClick: () -> Unit) {
    Box(
        Modifier
            .fillMaxWidth()
            .height(120.dp)
            .clip(RoundedCornerShape(28.dp))
            .border(1.dp, MaterialTheme.colorScheme.outline, RoundedCornerShape(28.dp))
            .clickable(onClick = onClick)
            .padding(18.dp),
    ) {
        Pill(stringResource(Res.string.installation_not_reached), leading = { Icon(FzIcons.CloudOff, null, Modifier.size(12.dp)) }, dark = false)
        Column(Modifier.align(Alignment.BottomStart)) {
            Text(inst.name, style = MaterialTheme.typography.titleMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
            Text(
                pluralStringResource(Res.plurals.installation_remote_zones, inst.zones.toInt(), inst.location ?: "—", inst.zones.toInt()),
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }
    }
}

@Composable
fun Pill(text: String, leading: (@Composable () -> Unit)? = null, dark: Boolean = true) {
    Row(
        Modifier
            .clip(CircleShape)
            .background(if (dark) Color.Black.copy(alpha = 0.25f) else MaterialTheme.colorScheme.surfaceVariant)
            .padding(horizontal = 10.dp, vertical = 5.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(6.dp),
    ) {
        leading?.invoke()
        Text(
            text,
            fontSize = 11.sp,
            fontWeight = FontWeight.Medium,
            color = if (dark) Color.White else MaterialTheme.colorScheme.onSurfaceVariant,
        )
    }
}

/**
 * Setpoint being edited: quick +/- taps or a drag accumulate here, and a single command is sent
 * once the user pauses (debounce).
 */
@Stable
class SetpointDraft(private val zone: () -> Zone) {
    var draft by mutableStateOf<Double?>(null)
    val value: Double get() = draft ?: zone().setpoint ?: 20.0
    fun nudge(delta: Double) {
        draft = clampSetpoint(zone(), value + delta)
    }
}

/**
 * One debounce effect for the whole lifetime of the draft (instead of restarting an effect on
 * every tap), and the latest `commit` callback is always used.
 */
@Composable
fun rememberSetpointDraft(zone: Zone, commit: (Double) -> Unit): SetpointDraft {
    val currentZone by rememberUpdatedState(zone)
    val currentCommit by rememberUpdatedState(commit)
    val draft = remember(zone.installationId, zone.systemId, zone.zoneId) { SetpointDraft { currentZone } }
    LaunchedEffect(draft) {
        snapshotFlow { draft.draft }.collectLatest { pending ->
            if (pending != null) {
                delay(700)
                draft.draft = null
                if (pending != currentZone.setpoint) currentCommit(pending)
            }
        }
    }
    return draft
}

@Composable
fun ZoneCard(zone: Zone, onToggle: (Boolean) -> Unit, onSetpoint: (Double) -> Unit, onClick: () -> Unit, modifier: Modifier = Modifier) {
    val tone = zone.tone()
    val card = Fz.colors.card
    // Animated border read in the draw phase only: the animation does not recompose the card.
    val border = animateColorAsState(if (zone.on) tone.copy(alpha = 0.55f) else MaterialTheme.colorScheme.outlineVariant)
    val draft = rememberSetpointDraft(zone, onSetpoint)
    val on = zone.on
    Column(
        modifier
            .clip(CardShape)
            .drawWithCache {
                val radius = CornerRadius(26.dp.toPx())
                val stroke = 1.dp.toPx()
                val glow = if (on) Brush.radialGradient(listOf(tone.copy(alpha = 0.18f), card), center = Offset(size.width, 0f), radius = size.maxDimension) else null
                onDrawBehind {
                    if (glow != null) drawRect(glow) else drawRect(card)
                    drawRoundRect(
                        color = border.value,
                        topLeft = Offset(stroke / 2, stroke / 2),
                        size = Size(size.width - stroke, size.height - stroke),
                        cornerRadius = radius,
                        style = Stroke(stroke),
                    )
                }
            }
            .clickable(onClick = onClick)
            .padding(14.dp),
    ) {
        Row(verticalAlignment = Alignment.Top) {
            Column(Modifier.weight(1f)) {
                Text(zone.name, style = MaterialTheme.typography.titleSmall, maxLines = 1, overflow = TextOverflow.Ellipsis)
                Row(verticalAlignment = Alignment.CenterVertically) {
                    if (zone.on && zone.mode != null) {
                        Icon(zone.mode.icon, null, tint = tone, modifier = Modifier.size(12.dp))
                        Spacer(Modifier.width(4.dp))
                        Text(
                            if (zone.demand) stringResource(Res.string.zone_demand) else zone.mode.label(),
                            color = tone,
                            fontSize = 11.sp,
                            fontWeight = FontWeight.Medium,
                        )
                    } else {
                        Text(stringResource(Res.string.zone_off), color = Fz.colors.muted, fontSize = 11.sp)
                    }
                    if (zone.batteryLow) {
                        Icon(FzIcons.BatteryWarning, stringResource(Res.string.zone_battery_low), tint = Fz.colors.warning, modifier = Modifier.padding(start = 4.dp).size(12.dp))
                    }
                    if (zone.errors.isNotEmpty()) {
                        Icon(FzIcons.TriangleAlert, stringResource(Res.string.zone_alert), tint = MaterialTheme.colorScheme.error, modifier = Modifier.padding(start = 4.dp).size(12.dp))
                    }
                }
            }
            Switch(
                checked = zone.on,
                onCheckedChange = onToggle,
                modifier = Modifier.height(28.dp),
                colors = SwitchDefaults.colors(checkedTrackColor = MaterialTheme.colorScheme.primary),
            )
        }
        Spacer(Modifier.height(10.dp))
        Text(formatTemp(zone.roomTemp), style = TempStyle, fontSize = 38.sp, letterSpacing = (-1).sp)
        if (zone.humidity != null) {
            Row(verticalAlignment = Alignment.CenterVertically) {
                Icon(FzIcons.Droplet, null, Modifier.size(11.dp), tint = Fz.colors.muted)
                Spacer(Modifier.width(3.dp))
                Text(stringResource(Res.string.zone_humidity, zone.humidity.toInt()), fontSize = 12.sp, color = Fz.colors.muted)
            }
        }
        Spacer(Modifier.height(10.dp))
        Row(
            Modifier
                .fillMaxWidth()
                .clip(RoundedCornerShape(14.dp))
                .background(MaterialTheme.colorScheme.surfaceVariant.copy(alpha = 0.6f))
                .alpha(if (zone.on) 1f else 0.6f),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.SpaceBetween,
        ) {
            IconButton(onClick = { draft.nudge(-zone.step) }, modifier = Modifier.size(36.dp)) {
                Icon(FzIcons.Minus, stringResource(Res.string.common_lower), Modifier.size(16.dp))
            }
            Text(
                formatTemp(draft.value),
                style = TempStyle,
                fontSize = 16.sp,
                color = if (draft.draft != null) tone else MaterialTheme.colorScheme.onSurface,
            )
            IconButton(onClick = { draft.nudge(zone.step) }, modifier = Modifier.size(36.dp)) {
                Icon(FzIcons.Plus, stringResource(Res.string.common_raise), Modifier.size(16.dp))
            }
        }
    }
}
