package com.breezone.app.ui.screens

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Scaffold
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.unit.dp
import com.breezone.app.data.CoverPositionState
import com.breezone.app.data.EnergyPowerState
import com.breezone.app.data.EvChargerState
import com.breezone.app.data.EvChargerStatus
import com.breezone.app.data.Hub
import com.breezone.app.data.LightState
import com.breezone.app.data.ServerDevices
import com.breezone.app.data.TraitIds
import com.breezone.app.data.VentilationRoomState
import com.breezone.app.data.WeatherView
import com.breezone.app.ui.components.WeatherCard
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import com.breezone.app.data.installation
import com.breezone.app.data.modulesHere
import com.breezone.app.resources.*
import com.breezone.app.ui.components.TypeTile
import com.breezone.app.ui.icons.FzIcons
import com.breezone.app.ui.installationPlace
import com.breezone.app.ui.theme.Fz
import org.jetbrains.compose.resources.pluralStringResource
import org.jetbrains.compose.resources.stringResource
import org.koin.compose.koinInject

/** The pages of an installation, by type of device, as in the mockups (« Maison »). */
enum class HouseType { Climate, Energy, Lights, Shutters, Ventilation, Solar, Charging }

/** The types of devices this server's modules have for an installation (climate excluded). */
fun ServerDevices.types(): List<HouseType> = buildList {
    // The energy page once a measure has a role (Settings › Modules on the web), as on the web.
    if (modules.any { it.role != null }) add(HouseType.Energy)
    if (withTrait(TraitIds.LIGHT).isNotEmpty()) add(HouseType.Lights)
    if (withTrait(TraitIds.COVER).isNotEmpty()) add(HouseType.Shutters)
    if (withTrait(TraitIds.VENTILATION_UNIT).isNotEmpty()) add(HouseType.Ventilation)
    if (withTrait(TraitIds.INVERTER).isNotEmpty()) add(HouseType.Solar)
    if (withTrait(TraitIds.CHARGER).isNotEmpty()) add(HouseType.Charging)
}

private data class Tile(val type: HouseType, val icon: ImageVector, val label: String, val sub: String, val color: Color)

@Composable
fun HouseScreen(serverId: String, installationId: Long, onBack: () -> Unit, onOpen: (HouseType) -> Unit) {
    val hub = koinInject<Hub>()
    val servers by hub.servers.collectAsState()
    val all by hub.devices.collectAsState()
    val server = servers.firstOrNull { it.account.id == serverId }
    val inst = servers.installation(serverId, installationId)
    val devices = all[serverId] ?: ServerDevices()
    val here = server?.modulesHere(inst) ?: false
    LaunchedEffect(serverId) { hub.followDevices(serverId, everyMs = 10_000) }
    var weather by remember { mutableStateOf<WeatherView?>(null) }
    LaunchedEffect(serverId) {
        while (true) {
            weather = runCatching { hub.api(serverId)?.weather() }.getOrNull() ?: weather
            kotlinx.coroutines.delay(10 * 60_000L)
        }
    }

    val zonesOn = inst?.zones?.count { it.on } ?: 0
    val tiles = buildList {
        add(Tile(HouseType.Climate, FzIcons.Thermometer, stringResource(Res.string.house_climate), pluralStringResource(Res.plurals.installation_zones_on, zonesOn, zonesOn, inst?.zones?.size ?: 0), Fz.colors.heat))
        if (here) devices.types().forEach { type ->
            when (type) {
                HouseType.Lights -> {
                    val lights = devices.withTrait(TraitIds.LIGHT)
                    val on = lights.count { it.available && it.stateAs(LightState.serializer())?.on == true }
                    add(Tile(type, FzIcons.Lightbulb, stringResource(Res.string.house_lights), pluralStringResource(Res.plurals.lights_on_of, on, on, lights.size), Fz.colors.light))
                }
                HouseType.Shutters -> {
                    val covers = devices.withTrait(TraitIds.COVER)
                    val open = covers.count { it.available && (it.stateAs(CoverPositionState.serializer())?.position ?: 0) > 0 }
                    add(Tile(type, FzIcons.Blinds, stringResource(Res.string.house_shutters), pluralStringResource(Res.plurals.shutters_open_of, open, open, covers.size), Fz.colors.shutter))
                }
                HouseType.Ventilation -> {
                    val level = ventilationLevel(devices.withTrait(TraitIds.VENTILATION_ROOM).mapNotNull { it.stateAs(VentilationRoomState.serializer()) })
                    add(Tile(type, FzIcons.Fan, stringResource(Res.string.house_ventilation), level?.let { stringResource(Res.string.ventilation_level_short, it) } ?: "–", Fz.colors.fan))
                }
                HouseType.Energy -> {
                    val solar = devices.modules.filter { it.role == "solar" }.sumOf { it.stateAs(EnergyPowerState.serializer())?.power ?: 0.0 }
                    val grid = devices.modules.filter { it.role == "grid" }.sumOf { it.stateAs(EnergyPowerState.serializer())?.power ?: 0.0 }
                    val sub = when {
                        solar > 0 -> stringResource(Res.string.energy_tile_solar, formatPower(solar))
                        devices.modules.any { it.role == "grid" } -> stringResource(Res.string.energy_tile_grid, formatPower(grid))
                        else -> stringResource(Res.string.energy_tile)
                    }
                    add(Tile(type, FzIcons.Zap, stringResource(Res.string.house_energy), sub, Fz.colors.warning))
                }
                HouseType.Solar -> {
                    val power = devices.withTrait(TraitIds.INVERTER).sumOf { it.stateAs(EnergyPowerState.serializer())?.power ?: 0.0 }
                    add(Tile(type, FzIcons.SolarPanel, stringResource(Res.string.house_solar), formatPower(power), Fz.colors.warning))
                }
                HouseType.Charging -> {
                    val c = devices.withTrait(TraitIds.CHARGER).first()
                    val state = c.stateAs(EvChargerState.serializer())
                    val power = c.stateAs(EnergyPowerState.serializer())?.power ?: 0.0
                    val sub = if (state?.status == EvChargerStatus.Charging) formatPower(power) else stringResource(
                        when (state?.status) {
                            EvChargerStatus.Plugged -> Res.string.charger_plugged
                            EvChargerStatus.Fault -> Res.string.charger_fault
                            else -> Res.string.charger_available
                        },
                    )
                    add(Tile(type, FzIcons.PlugZap, stringResource(Res.string.house_charging), sub, MaterialTheme.colorScheme.primary))
                }
                HouseType.Climate -> Unit
            }
        }
    }

    Scaffold(topBar = {
        FzTopBar(inst?.name ?: stringResource(Res.string.house_title), inst?.let { installationPlace(it) }, onBack)
    }) { padding ->
        LazyColumn(
            Modifier.fillMaxSize(),
            contentPadding = PaddingValues(start = 16.dp, end = 16.dp, top = padding.calculateTopPadding() + 4.dp, bottom = 24.dp),
            verticalArrangement = Arrangement.spacedBy(12.dp),
        ) {
            // The weather is the home's: shown with this server's installations.
            if (here) item(key = "weather", contentType = "weather") { WeatherCard(weather) }
            items(tiles.chunked(2), key = { row -> row.first().type.name }, contentType = { "tiles" }) { row ->
                Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(12.dp)) {
                    row.forEach { t ->
                        TypeTile(t.icon, t.label, t.sub, t.color, onClick = { onOpen(t.type) }, modifier = Modifier.weight(1f))
                    }
                    if (row.size == 1) Spacer(Modifier.weight(1f))
                }
            }
            if (!here && inst != null) {
                item(key = "elsewhere", contentType = "note") {
                    SectionCard {
                        Column(verticalArrangement = Arrangement.spacedBy(4.dp)) {
                            Muted(stringResource(Res.string.house_other_server, inst.name))
                        }
                    }
                }
            }
        }
    }
}

/** The level of the house: the air flow of every valve over their nominal flows. */
fun ventilationLevel(rooms: List<VentilationRoomState>): Int? {
    val nominal = rooms.sumOf { it.nominal ?: 0.0 }
    if (nominal <= 0) return null
    return (rooms.sumOf { it.flow } / nominal * 100).toInt()
}
