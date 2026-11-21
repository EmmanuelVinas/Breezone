package com.breezone.app.ui.home

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Switch
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.breezone.app.data.ButtonState
import com.breezone.app.data.ChargerShown
import com.breezone.app.data.DeviceView
import com.breezone.app.data.EnergyPowerState
import com.breezone.app.data.EvChargerState
import com.breezone.app.data.Hub
import com.breezone.app.data.SwitchRelayState
import com.breezone.app.data.TRAIT_KINDS
import com.breezone.app.data.VentilationUnitState
import com.breezone.app.data.Zone
import com.breezone.app.data.ZoneCommand
import com.breezone.app.data.commandKey
import com.breezone.app.data.shown
import com.breezone.app.data.zoneRef
import com.breezone.app.resources.*
import com.breezone.app.ui.components.CommandIndicator
import com.breezone.app.ui.components.IconBadge
import com.breezone.app.ui.formatTemp
import com.breezone.app.ui.icons.FzIcons
import com.breezone.app.ui.label
import com.breezone.app.ui.screens.LightRow
import com.breezone.app.ui.screens.ShutterRow
import com.breezone.app.ui.screens.ValveRow
import com.breezone.app.ui.screens.chargerLabel
import com.breezone.app.ui.screens.formatPower
import com.breezone.app.ui.screens.setLight
import com.breezone.app.ui.theme.Fz
import com.breezone.app.ui.tone
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.put
import org.jetbrains.compose.resources.StringResource
import org.jetbrains.compose.resources.stringResource

/**
 * A kind of device, as cards and lists group them (the web's `deviceType`, same order): its name,
 * its icon, and the page of the bar that handles it.
 */
enum class DeviceKind(val id: String, val label: StringResource, val icon: ImageVector, val page: AppPage?) {
    Climate("climate", Res.string.house_climate, FzIcons.Thermometer, AppPage.Climate),
    Charger("charger", Res.string.house_charging, FzIcons.PlugZap, AppPage.Charging),
    Light("light", Res.string.house_lights, FzIcons.Lightbulb, AppPage.Lights),
    Shutter("shutter", Res.string.house_shutters, FzIcons.Blinds, AppPage.Shutters),
    Window("window", Res.string.kind_windows, FzIcons.AppWindow, AppPage.Shutters),
    Door("door", Res.string.kind_doors, FzIcons.DoorOpen, AppPage.Shutters),
    Ventilation("ventilation", Res.string.house_ventilation, FzIcons.Fan, AppPage.Ventilation),
    RoomAir("room-air", Res.string.kind_room_air, FzIcons.Wind, AppPage.Ventilation),
    Switch("switch", Res.string.kind_switches, FzIcons.Power, null),
    Solar("solar", Res.string.house_solar, FzIcons.SolarPanel, AppPage.Solar),
    Energy("energy", Res.string.house_energy, FzIcons.Zap, AppPage.Energy),
    Sensor("sensor", Res.string.kind_sensors, FzIcons.Thermometer, null),
    Remote("remote", Res.string.kind_remotes, FzIcons.Gamepad2, null),
    Other("other", Res.string.kind_other, FzIcons.Box, null),
    ;

    companion object {
        fun byId(id: String?) = entries.firstOrNull { it.id == id }
    }
}

/**
 * The kind of a device: its own kind when it names one (a shutter, a window…), else the first in
 * list order among those of its traits (`ui.kind` in the catalog), else Other.
 */
val DeviceView.deviceKind: DeviceKind
    get() {
        DeviceKind.byId(kind)?.takeIf { it != DeviceKind.Other }?.let { return it }
        return (listOf(kind) + traits.map { it.substringBefore('@') })
            .mapNotNull { TRAIT_KINDS[it]?.let(DeviceKind::byId) }
            .minByOrNull { it.ordinal } ?: DeviceKind.Other
    }

@Composable
fun DeviceKind.color(): Color = when (this) {
    DeviceKind.Climate -> Fz.colors.heat
    DeviceKind.Light -> Fz.colors.light
    DeviceKind.Shutter, DeviceKind.Window, DeviceKind.Door -> Fz.colors.shutter
    DeviceKind.Ventilation, DeviceKind.RoomAir, DeviceKind.Sensor -> Fz.colors.air
    DeviceKind.Solar, DeviceKind.Energy, DeviceKind.Switch -> Fz.colors.warning
    else -> MaterialTheme.colorScheme.primary
}

/** The zone of the installation a climate device is. */
fun HomeContext.zoneOf(d: DeviceView): Zone? {
    val (system, zone) = d.zoneRef() ?: return null
    return installation?.zones?.firstOrNull { it.systemId == system && it.zoneId == zone }
}

/**
 * A device on a line of a card, driven from there: the row of its own page when it has one (a light
 * and its brightness, a shutter and its buttons, a room's valve), else its state and its switch.
 */
@Composable
fun DeviceEntry(d: DeviceView, ctx: HomeContext) {
    val hub = ctx.hub
    val status = ctx.deviceStatus["${ctx.serverId}/${d.id}"]
    val open = { ctx.nav.openDevice(d) }
    when (d.deviceKind) {
        DeviceKind.Light -> LightRow(d, status, open) { hub.setLight(ctx.serverId, d, it) }
        DeviceKind.Shutter, DeviceKind.Window, DeviceKind.Door -> ShutterRow(d, status, open, hub, ctx.serverId)
        DeviceKind.RoomAir -> ValveRow(d, demanding = false, cause = "", onClick = open)
        DeviceKind.Climate -> {
            val zone = ctx.zoneOf(d)
            if (zone != null) ZoneLine(zone, ctx) else InfoLine(d, ctx)
        }
        DeviceKind.Switch -> {
            val on = d.available && d.stateAs(SwitchRelayState.serializer())?.on == true
            val power = d.stateAs(EnergyPowerState.serializer())?.power
            DeviceLine(
                icon = FzIcons.Power,
                color = Fz.colors.warning,
                active = on,
                name = d.name,
                text = when {
                    !d.available -> stringResource(Res.string.device_unavailable)
                    on -> listOfNotNull(stringResource(Res.string.switch_on), power?.takeIf { it > 0 }?.let(::formatPower)).joinToString(" · ")
                    else -> stringResource(Res.string.switch_off)
                },
                onClick = null,
            ) {
                CommandIndicator(status, Modifier.padding(end = 6.dp))
                Switch(checked = on, enabled = d.usable, onCheckedChange = { v ->
                    hub.act(ctx.serverId, d, "switch.relay", "set", buildJsonObject { put("on", v) }, optimistic = buildJsonObject { put("on", v) })
                })
            }
        }
        else -> InfoLine(d, ctx)
    }
}

/** A zone of the climate on a line: its temperature, its setpoint, and its switch. */
@Composable
fun ZoneLine(zone: Zone, ctx: HomeContext) {
    val status = ctx.commandStatus[commandKey(ctx.serverId, zone)]
    DeviceLine(
        icon = FzIcons.Thermometer,
        color = zone.tone(),
        active = zone.on,
        name = zone.name,
        text = if (zone.on) {
            listOfNotNull(zone.mode?.label(), "${formatTemp(zone.roomTemp)} → ${formatTemp(zone.setpoint)}").joinToString(" · ")
        } else {
            listOf(stringResource(Res.string.zone_off), formatTemp(zone.roomTemp)).joinToString(" · ")
        },
        onClick = { ctx.nav.openZone(zone) },
    ) {
        CommandIndicator(status, Modifier.padding(end = 6.dp))
        Switch(checked = zone.on, enabled = ctx.installation?.controllable == true, onCheckedChange = { ctx.hub.control(ctx.serverId, zone, ZoneCommand(on = it)) })
    }
}

/** A device only looked at here: what it does now, and its page. */
@Composable
private fun InfoLine(d: DeviceView, ctx: HomeContext) {
    val kind = d.deviceKind
    val power = d.stateAs(EnergyPowerState.serializer())?.power
    val (text, active) = when {
        !d.available -> stringResource(Res.string.device_unavailable) to false
        kind == DeviceKind.Charger -> {
            val shown = d.stateAs(EvChargerState.serializer())?.shown ?: ChargerShown.Available
            listOfNotNull(chargerLabel(shown), power?.takeIf { shown == ChargerShown.Charging && it > 0 }?.let(::formatPower)).joinToString(" · ") to (shown == ChargerShown.Charging)
        }
        kind == DeviceKind.Ventilation -> {
            val flow = d.stateAs(VentilationUnitState.serializer())?.flow
            (flow?.let { stringResource(Res.string.ventilation_flow, it.toInt()) } ?: "–") to true
        }
        kind == DeviceKind.Remote -> {
            val b = d.stateAs(ButtonState.serializer())
            (b?.lastButton?.let { stringResource(Res.string.remote_last_button, it.toInt()) } ?: stringResource(Res.string.remote_no_press)) to false
        }
        power != null -> formatPower(power) to (power > 0)
        else -> "" to false
    }
    DeviceLine(kind.icon, kind.color(), active, d.name, text.ifEmpty { null }, onClick = kind.page?.let { { ctx.nav.openDevice(d) } })
}

/** A line of a card: an icon, a name and what it is now, then its controls. */
@Composable
fun DeviceLine(
    icon: ImageVector,
    color: Color,
    active: Boolean,
    name: String,
    text: String?,
    onClick: (() -> Unit)?,
    trailing: @Composable () -> Unit = {
        if (onClick != null) Icon(FzIcons.ChevronRight, null, Modifier.size(18.dp), tint = Fz.colors.muted)
    },
) {
    Row(
        Modifier.fillMaxWidth().let { if (onClick != null) it.clickable(onClick = onClick) else it }.padding(vertical = 8.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        IconBadge(icon, color, active = active)
        Spacer(Modifier.width(12.dp))
        Column(Modifier.weight(1f)) {
            Text(name, style = MaterialTheme.typography.bodyLarge, maxLines = 1, overflow = TextOverflow.Ellipsis)
            if (text != null) Text(text, fontSize = 12.sp, color = if (active) color else Fz.colors.muted, maxLines = 1, overflow = TextOverflow.Ellipsis)
        }
        trailing()
    }
}
