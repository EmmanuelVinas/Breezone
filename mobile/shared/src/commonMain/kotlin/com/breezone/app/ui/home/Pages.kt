package com.breezone.app.ui.home

import androidx.compose.runtime.Composable
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.vector.ImageVector
import com.breezone.app.data.Installation
import com.breezone.app.data.ServerDevices
import com.breezone.app.data.TraitIds
import com.breezone.app.resources.Res
import com.breezone.app.resources.house_charging
import com.breezone.app.resources.house_climate
import com.breezone.app.resources.house_energy
import com.breezone.app.resources.house_lights
import com.breezone.app.resources.house_shutters
import com.breezone.app.resources.house_solar
import com.breezone.app.resources.house_title
import com.breezone.app.resources.house_ventilation
import com.breezone.app.resources.installation_tab_routines
import com.breezone.app.resources.installation_tab_schedules
import com.breezone.app.resources.tab_settings
import com.breezone.app.ui.icons.FzIcons
import com.breezone.app.ui.theme.Fz
import org.jetbrains.compose.resources.StringResource

/** What decides which pages an installation has: its zones, and the devices of its server's modules. */
data class PageContext(
    val installation: Installation?,
    val devices: ServerDevices,
    /** The modules of the server go with this installation (not one reached through another server). */
    val modulesHere: Boolean,
)

/**
 * A page of the app, as an entry of the bottom bar. Each keeps its own screen; the bar shows those
 * the installation has, in this order.
 */
enum class AppPage(
    val label: StringResource,
    val icon: ImageVector,
    /** Whether the installation has this page (the home and the settings always). */
    val shown: (PageContext) -> Boolean,
) {
    Home(Res.string.house_title, FzIcons.House, { true }),
    Climate(Res.string.house_climate, FzIcons.Thermometer, { it.installation?.airzone == true }),
    Energy(Res.string.house_energy, FzIcons.Zap, { it.modulesHere && it.devices.modules.any { d -> d.role != null } }),
    Solar(Res.string.house_solar, FzIcons.SolarPanel, { it.has(TraitIds.INVERTER) || it.has(TraitIds.FORECAST) }),
    Charging(Res.string.house_charging, FzIcons.PlugZap, { it.has(TraitIds.CHARGER) }),
    Lights(Res.string.house_lights, FzIcons.Lightbulb, { it.has(TraitIds.LIGHT) }),
    Shutters(Res.string.house_shutters, FzIcons.Blinds, { it.has(TraitIds.COVER) }),
    Ventilation(Res.string.house_ventilation, FzIcons.Fan, { it.has(TraitIds.VENTILATION_UNIT) || it.has(TraitIds.VENTILATION_ROOM) }),
    Routines(Res.string.installation_tab_routines, FzIcons.Sparkles, { it.installation != null }),
    Schedules(Res.string.installation_tab_schedules, FzIcons.CalendarClock, { it.installation != null }),
    Settings(Res.string.tab_settings, FzIcons.Settings, { true }),
    ;

    companion object {
        fun of(ctx: PageContext) = entries.filter { it.shown(ctx) }
    }
}

private fun PageContext.has(trait: String) = modulesHere && devices.withTrait(trait).isNotEmpty()

/** The accent of a page, as its tile on the home shows it. */
@Composable
fun AppPage.color(): Color = when (this) {
    AppPage.Climate -> Fz.colors.heat
    AppPage.Energy, AppPage.Solar -> Fz.colors.warning
    AppPage.Lights -> Fz.colors.light
    AppPage.Shutters -> Fz.colors.shutter
    AppPage.Ventilation -> Fz.colors.air
    else -> androidx.compose.material3.MaterialTheme.colorScheme.primary
}
