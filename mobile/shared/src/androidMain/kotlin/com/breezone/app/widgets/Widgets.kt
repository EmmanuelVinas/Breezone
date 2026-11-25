package com.breezone.app.widgets

import android.appwidget.AppWidgetManager
import android.content.ComponentName
import android.content.Context
import android.content.Intent
import android.net.Uri
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.datastore.preferences.core.longPreferencesKey
import androidx.glance.currentState
import androidx.glance.appwidget.state.updateAppWidgetState
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.unit.TextUnit
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.glance.GlanceId
import androidx.glance.ColorFilter
import androidx.glance.GlanceModifier
import androidx.glance.Image
import androidx.glance.ImageProvider
import androidx.glance.LocalContext
import androidx.glance.action.ActionParameters
import androidx.glance.action.actionParametersOf
import androidx.glance.action.actionStartActivity
import androidx.glance.action.clickable
import androidx.glance.appwidget.GlanceAppWidget
import androidx.glance.appwidget.GlanceAppWidgetManager
import androidx.glance.appwidget.GlanceAppWidgetReceiver
import androidx.glance.appwidget.action.ActionCallback
import androidx.glance.appwidget.action.actionRunCallback
import androidx.glance.appwidget.action.actionStartActivity as startActivityIntent
import androidx.glance.appwidget.cornerRadius
import androidx.glance.appwidget.provideContent
import androidx.glance.background
import androidx.glance.color.ColorProvider
import androidx.glance.layout.Alignment
import androidx.glance.layout.Box
import androidx.glance.layout.Column
import androidx.glance.layout.Row
import androidx.glance.layout.Spacer
import androidx.glance.layout.fillMaxSize
import androidx.glance.layout.fillMaxWidth
import androidx.glance.layout.height
import androidx.glance.layout.padding
import androidx.glance.layout.size
import androidx.glance.layout.width
import androidx.glance.text.FontWeight
import androidx.glance.text.Text
import androidx.glance.text.TextStyle
import androidx.glance.unit.ColorProvider
import com.breezone.app.data.DayColor
import com.breezone.app.data.DeviceActionInput
import com.breezone.app.data.EnergyLive
import com.breezone.app.data.LightState
import com.breezone.app.data.PriceView
import com.breezone.app.data.Routine
import com.breezone.app.data.Zone
import com.breezone.app.data.ZoneCommand
import com.breezone.app.shared.R
import com.breezone.app.ui.theme.WidgetPalette
import kotlinx.coroutines.coroutineScope
import kotlinx.coroutines.launch
import kotlinx.coroutines.withTimeoutOrNull
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.booleanOrNull
import kotlinx.serialization.json.jsonPrimitive
import kotlin.math.abs
import kotlin.math.roundToInt

private fun color(pair: Pair<Color, Color>): ColorProvider = ColorProvider(day = pair.first, night = pair.second)

private val Background get() = color(WidgetPalette.background)
private val TextColor get() = color(WidgetPalette.text)
private val Muted get() = color(WidgetPalette.muted)
private val Primary get() = color(WidgetPalette.primary)
private val OnPrimary get() = color(WidgetPalette.onPrimary)
private val Button get() = color(WidgetPalette.button)

private fun style(size: TextUnit = 13.sp, color: ColorProvider = TextColor, bold: Boolean = false) =
    TextStyle(color = color, fontSize = size, fontWeight = if (bold) FontWeight.Bold else FontWeight.Normal)

/** "2,3" (one decimal, the separator of the language). */
private fun decimal(value: Double, digits: Int = 1): String {
    val factor = if (digits == 2) 100.0 else 10.0
    val text = ((value * factor).roundToInt() / factor).toString()
    val separator = java.text.DecimalFormatSymbols.getInstance().decimalSeparator
    return text.replace('.', separator)
}

private fun kw(watts: Double) = "${decimal(watts / 1000)} kW"

private fun temp(value: Double?) = value?.let { "${decimal(it)}°" } ?: "—"

private val ConfigKey = ActionParameters.Key<Int>("widget")
private val IdKey = ActionParameters.Key<Long>("id")
private val DeltaKey = ActionParameters.Key<Double>("delta")
private val OnKey = ActionParameters.Key<Boolean>("on")
private val TraitKey = ActionParameters.Key<String>("trait")

/** A widget of Breezone: what it shows is read from the server at each update. */
abstract class BreezoneWidget(val kind: WidgetKind) : GlanceAppWidget() {
    /** What the widget shows (`null`: the server cannot be read now). */
    abstract suspend fun load(context: Context, config: WidgetConfig): Any?

    @Composable
    abstract fun Content(data: Any, config: WidgetConfig, appWidgetId: Int)

    override suspend fun provideGlance(context: Context, id: GlanceId) {
        val appWidgetId = GlanceAppWidgetManager(context).getAppWidgetId(id)
        provideContent {
            // Read again whenever the version of the widget changes (a choice, an action).
            val version = currentState(VersionKey) ?: 0L
            var shown by remember { mutableStateOf<Pair<WidgetConfig?, Any?>?>(null) }
            LaunchedEffect(version) {
                val config = WidgetStore.get(context, appWidgetId)
                val data = config?.let { c -> withTimeoutOrNull(15_000) { runCatching { load(context, c) }.getOrNull() } }
                shown = config to data
            }
            val (config, data) = shown ?: (WidgetStore.get(context, appWidgetId) to null)
            Frame(appWidgetId, configured = config != null) {
                when {
                    config == null -> Message(context.getString(R.string.widget_choose))
                    api(config.serverId) == null -> Message(context.getString(R.string.widget_sign_in))
                    shown == null -> Message(config.title)
                    data == null -> Message(context.getString(R.string.widget_unreachable, config.title))
                    else -> Content(data, config, appWidgetId)
                }
            }
        }
    }
}

private val VersionKey = longPreferencesKey("version")

/** Shows a widget again, read anew from its server. */
internal suspend fun BreezoneWidget.refresh(context: Context, id: GlanceId) {
    updateAppWidgetState(context, id) { it[VersionKey] = (it[VersionKey] ?: 0L) + 1 }
    update(context, id)
}

/** The card of a widget; a touch outside its buttons opens the app (or the choice, when not made). */
@Composable
private fun Frame(appWidgetId: Int, configured: Boolean, content: @Composable () -> Unit) {
    val context = LocalContext.current
    val open = if (configured) {
        actionStartActivity(ComponentName(context.packageName, "com.breezone.app.MainActivity"))
    } else {
        startActivityIntent(
            Intent(context, WidgetConfigActivity::class.java)
                .putExtra(AppWidgetManager.EXTRA_APPWIDGET_ID, appWidgetId)
                .setData(Uri.parse("breezone-widget://$appWidgetId")),
        )
    }
    Box(GlanceModifier.fillMaxSize().background(Background).cornerRadius(24.dp).padding(12.dp).clickable(open)) { content() }
}

@Composable
private fun Message(text: String) {
    Box(GlanceModifier.fillMaxSize(), contentAlignment = Alignment.Center) { Text(text, style = style(color = Muted)) }
}

@Composable
private fun RoundButton(label: String, onClick: androidx.glance.action.Action, highlighted: Boolean = false) {
    Box(
        GlanceModifier.size(36.dp).cornerRadius(18.dp).background(if (highlighted) Primary else Button).clickable(onClick),
        contentAlignment = Alignment.Center,
    ) { Text(label, style = style(16.sp, if (highlighted) OnPrimary else TextColor, bold = true)) }
}

/** The on/off button. */
@Composable
private fun PowerButton(on: Boolean, onClick: androidx.glance.action.Action) {
    Box(
        GlanceModifier.size(36.dp).cornerRadius(18.dp).background(if (on) Primary else Button).clickable(onClick),
        contentAlignment = Alignment.Center,
    ) {
        Image(
            ImageProvider(R.drawable.ic_widget_power),
            contentDescription = LocalContext.current.getString(if (on) R.string.widget_on else R.string.widget_off),
            modifier = GlanceModifier.size(18.dp),
            colorFilter = ColorFilter.tint(if (on) OnPrimary else TextColor),
        )
    }
}

// ---------------------------------------------------------------------------------------
// A zone of the air conditioning
// ---------------------------------------------------------------------------------------

class ZoneWidget : BreezoneWidget(WidgetKind.Zone) {
    override suspend fun load(context: Context, config: WidgetConfig): Zone? =
        api(config.serverId)?.status()?.firstOrNull { it.id == config.installationId }?.zones?.firstOrNull { it.systemId == config.systemId && it.zoneId == config.zoneId }

    @Composable
    override fun Content(data: Any, config: WidgetConfig, appWidgetId: Int) {
        val zone = data as Zone
        val context = LocalContext.current
        val params = { delta: Double?, on: Boolean? ->
            actionParametersOf(
                ConfigKey to appWidgetId,
                *listOfNotNull(delta?.let { DeltaKey to it }, on?.let { OnKey to it }).toTypedArray(),
            )
        }
        Column(GlanceModifier.fillMaxSize()) {
            Text(zone.name, style = style(12.sp, Muted), maxLines = 1)
            Text(temp(zone.roomTemp), style = style(26.sp, bold = true))
            Text(
                if (zone.on) context.getString(R.string.widget_zone_setpoint, temp(zone.setpoint)) else context.getString(R.string.widget_off),
                style = style(12.sp, Muted),
                maxLines = 1,
            )
            Spacer(GlanceModifier.defaultWeight())
            Row(GlanceModifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
                RoundButton("−", actionRunCallback<ZoneAction>(params(-zone.step, null)))
                Spacer(GlanceModifier.width(8.dp))
                RoundButton("+", actionRunCallback<ZoneAction>(params(zone.step, null)))
                Spacer(GlanceModifier.defaultWeight())
                PowerButton(zone.on, actionRunCallback<ZoneAction>(params(null, !zone.on)))
            }
        }
    }
}

class ZoneAction : ActionCallback {
    override suspend fun onAction(context: Context, glanceId: GlanceId, parameters: ActionParameters) {
        val config = WidgetStore.get(context, parameters[ConfigKey] ?: return) ?: return
        val api = api(config.serverId) ?: return
        val zone = runCatching { ZoneWidget().load(context, config) }.getOrNull() ?: return
        val cmd = parameters[OnKey]?.let { ZoneCommand(on = it) } ?: parameters[DeltaKey]?.let { d ->
            val now = zone.setpoint ?: return@let null
            val lo = zone.minTemp ?: 15.0
            val hi = zone.maxTemp ?: 30.0
            ZoneCommand(setpoint = (now + d).coerceIn(lo, hi))
        } ?: return
        runCatching { api.control(zone, cmd) }
        ZoneWidget().refresh(context, glanceId)
    }
}

// ---------------------------------------------------------------------------------------
// Energy now
// ---------------------------------------------------------------------------------------

class EnergyWidget : BreezoneWidget(WidgetKind.Energy) {
    override suspend fun load(context: Context, config: WidgetConfig): EnergyLive? = api(config.serverId)?.energyLive()

    @Composable
    override fun Content(data: Any, config: WidgetConfig, appWidgetId: Int) {
        val live = data as EnergyLive
        val context = LocalContext.current
        val cells = listOfNotNull(
            live.solar?.takeIf { live.roles.solar }?.let { Triple(context.getString(R.string.widget_solar), kw(it), color(WidgetPalette.solar)) },
            live.home?.let { Triple(context.getString(R.string.widget_home), kw(it), TextColor) },
            live.grid?.takeIf { live.roles.grid }?.let { g -> Triple(context.getString(R.string.widget_grid), kw(g), if (g < 0) color(WidgetPalette.export) else TextColor) },
            live.battery?.takeIf { live.roles.battery }?.let { Triple(context.getString(R.string.widget_battery), kw(it), TextColor) },
        )
        Column(GlanceModifier.fillMaxSize()) {
            Text(context.getString(R.string.widget_energy_title, config.title), style = style(12.sp, Muted), maxLines = 1)
            Spacer(GlanceModifier.height(6.dp))
            if (cells.isEmpty()) Text(context.getString(R.string.widget_no_measure), style = style(color = Muted))
            Row(GlanceModifier.fillMaxWidth()) {
                cells.forEach { (label, value, tone) ->
                    Column(GlanceModifier.defaultWeight()) {
                        Text(label, style = style(11.sp, Muted))
                        Text(value, style = style(18.sp, tone, bold = true), maxLines = 1)
                    }
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------------------
// The price of electricity
// ---------------------------------------------------------------------------------------

class PriceWidget : BreezoneWidget(WidgetKind.Price) {
    override suspend fun load(context: Context, config: WidgetConfig): PriceView? = config.installationId?.let { api(config.serverId)?.priceToday(it) }

    @Composable
    override fun Content(data: Any, config: WidgetConfig, appWidgetId: Int) {
        val price = data as PriceView
        val context = LocalContext.current
        val now = price.now
        val currency = if (price.currency == "EUR") "€" else price.currency
        val tempo = price.tempo?.today?.let {
            context.getString(
                when (it) {
                    DayColor.Blue -> R.string.widget_tempo_blue
                    DayColor.White -> R.string.widget_tempo_white
                    DayColor.Red -> R.string.widget_tempo_red
                },
            )
        }
        Column(GlanceModifier.fillMaxSize()) {
            Text(context.getString(R.string.widget_price_title), style = style(12.sp, Muted), maxLines = 1)
            if (now == null) {
                Text(context.getString(R.string.widget_no_tariff), style = style(color = Muted))
            } else {
                Text("${decimal(now.price, 2)} $currency", style = style(24.sp, bold = true))
                Text(listOfNotNull(context.getString(R.string.widget_per_kwh), now.slot.takeIf { it.isNotBlank() }, tempo).joinToString(" · "), style = style(12.sp, Muted), maxLines = 1)
            }
            Spacer(GlanceModifier.defaultWeight())
            // The day: cheaper stretches in green, dearer ones in yellow.
            val periods = price.periods
            if (periods.isNotEmpty()) {
                val low = periods.minOf { it.price }
                val span = (periods.last().end - periods.first().start).coerceAtLeast(1)
                Row(GlanceModifier.fillMaxWidth().height(8.dp).cornerRadius(4.dp)) {
                    periods.forEach { p ->
                        val cheap = abs(p.price - low) < 1e-6
                        Box(
                            GlanceModifier.width((((p.end - p.start).toDouble() / span) * 150).dp).height(8.dp)
                                .background(color(if (cheap) WidgetPalette.export else WidgetPalette.solar)),
                        ) {}
                    }
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------------------
// A device: a light, a plug, a charger
// ---------------------------------------------------------------------------------------

class DeviceWidget : BreezoneWidget(WidgetKind.Device) {
    override suspend fun load(context: Context, config: WidgetConfig): Any? {
        val api = api(config.serverId) ?: return null
        if (!config.lightGroup) return api.devices().firstOrNull { it.id == config.deviceId }
        val lights = api.devices().lightsOf(config)
        val name = config.roomId?.let { id -> runCatching { api.rooms() }.getOrNull()?.firstOrNull { it.id == id }?.name } ?: config.title
        return LightGroup(name, lights)
    }

    @Composable
    override fun Content(data: Any, config: WidgetConfig, appWidgetId: Int) {
        if (data is LightGroup) return Group(data, appWidgetId)
        val device = data as com.breezone.app.data.DeviceView
        val context = LocalContext.current
        val trait = device.widgetTrait()
        val state = device.state ?: JsonObject(emptyMap())
        val on = state["on"]?.jsonPrimitive?.booleanOrNull
        val subtitle = when {
            trait == "ev.charger" -> when (state["status"]?.jsonPrimitive?.content) {
                "charging" -> context.getString(R.string.widget_charger_charging)
                "plugged" -> context.getString(R.string.widget_charger_plugged)
                "fault" -> context.getString(R.string.widget_charger_fault)
                else -> context.getString(R.string.widget_charger_available)
            }
            on == true -> context.getString(R.string.widget_on)
            else -> context.getString(R.string.widget_off)
        }
        Row(GlanceModifier.fillMaxSize(), verticalAlignment = Alignment.CenterVertically) {
            Column(GlanceModifier.defaultWeight()) {
                Text(device.name, style = style(14.sp, bold = true), maxLines = 1)
                Text(subtitle, style = style(12.sp, Muted), maxLines = 1)
            }
            if (on != null && trait != "ev.charger" && device.canControl != false) {
                PowerButton(on, actionRunCallback<DeviceAction>(actionParametersOf(ConfigKey to appWidgetId, OnKey to !on, TraitKey to trait!!)))
            }
        }
    }

    /** The lights of a room (or of the home): on when any of them is; the button switches them all. */
    @Composable
    private fun Group(group: LightGroup, appWidgetId: Int) {
        val context = LocalContext.current
        val lit = group.lights.count { it.available && it.stateAs(LightState.serializer())?.on == true }
        Row(GlanceModifier.fillMaxSize(), verticalAlignment = Alignment.CenterVertically) {
            Column(GlanceModifier.defaultWeight()) {
                Text(group.name, style = style(14.sp, bold = true), maxLines = 1)
                Text(context.resources.getQuantityString(R.plurals.widget_lights_on_of, lit, lit, group.lights.size), style = style(12.sp, Muted), maxLines = 1)
            }
            if (group.lights.any { it.switchable() }) {
                PowerButton(lit > 0, actionRunCallback<DeviceAction>(actionParametersOf(ConfigKey to appWidgetId, OnKey to (lit == 0), TraitKey to "light")))
            }
        }
    }
}

private fun com.breezone.app.data.DeviceView.switchable() = usable && stateAs(LightState.serializer())?.reachable != false

class DeviceAction : ActionCallback {
    override suspend fun onAction(context: Context, glanceId: GlanceId, parameters: ActionParameters) {
        val config = WidgetStore.get(context, parameters[ConfigKey] ?: return) ?: return
        val api = api(config.serverId) ?: return
        val on = parameters[OnKey] ?: return
        val trait = parameters[TraitKey] ?: return
        val set = DeviceActionInput(trait, "set", JsonObject(mapOf("on" to JsonPrimitive(on))))
        if (config.lightGroup) {
            val lights = runCatching { api.devices().lightsOf(config) }.getOrDefault(emptyList()).filter { it.switchable() }
            coroutineScope { lights.forEach { d -> launch { runCatching { api.act(d.id, set) } } } }
        } else {
            val id = config.deviceId ?: return
            runCatching { api.act(id, set) }
        }
        DeviceWidget().refresh(context, glanceId)
    }
}

// ---------------------------------------------------------------------------------------
// Scenes (routines)
// ---------------------------------------------------------------------------------------

class ScenesWidget : BreezoneWidget(WidgetKind.Scenes) {
    override suspend fun load(context: Context, config: WidgetConfig): List<Routine>? = api(config.serverId)?.routines()?.filter { it.enabled }?.take(4)

    @Composable
    override fun Content(data: Any, config: WidgetConfig, appWidgetId: Int) {
        @Suppress("UNCHECKED_CAST")
        val routines = data as List<Routine>
        val context = LocalContext.current
        Column(GlanceModifier.fillMaxSize()) {
            Text(context.getString(R.string.widget_scenes_title), style = style(12.sp, Muted))
            Spacer(GlanceModifier.height(6.dp))
            if (routines.isEmpty()) Text(context.getString(R.string.widget_no_scene), style = style(color = Muted))
            routines.chunked(2).forEach { row ->
                Row(GlanceModifier.fillMaxWidth().padding(bottom = 6.dp)) {
                    row.forEachIndexed { i, r ->
                        if (i > 0) Spacer(GlanceModifier.width(6.dp))
                        Box(
                            GlanceModifier.defaultWeight().height(36.dp).cornerRadius(18.dp).background(Button)
                                .clickable(actionRunCallback<SceneAction>(actionParametersOf(ConfigKey to appWidgetId, IdKey to r.id))),
                            contentAlignment = Alignment.Center,
                        ) { Text(r.name, style = style(13.sp), maxLines = 1) }
                    }
                }
            }
        }
    }
}

class SceneAction : ActionCallback {
    override suspend fun onAction(context: Context, glanceId: GlanceId, parameters: ActionParameters) {
        val config = WidgetStore.get(context, parameters[ConfigKey] ?: return) ?: return
        val id = parameters[IdKey] ?: return
        runCatching { api(config.serverId)?.runRoutine(id) }
        ScenesWidget().refresh(context, glanceId)
    }
}

// ---------------------------------------------------------------------------------------
// Receivers
// ---------------------------------------------------------------------------------------

abstract class BreezoneWidgetReceiver(widget: BreezoneWidget) : GlanceAppWidgetReceiver() {
    override val glanceAppWidget: GlanceAppWidget = widget

    override fun onDeleted(context: Context, appWidgetIds: IntArray) {
        super.onDeleted(context, appWidgetIds)
        appWidgetIds.forEach { WidgetStore.remove(context, it) }
    }
}

class ZoneWidgetReceiver : BreezoneWidgetReceiver(ZoneWidget())
class EnergyWidgetReceiver : BreezoneWidgetReceiver(EnergyWidget())
class PriceWidgetReceiver : BreezoneWidgetReceiver(PriceWidget())
class DeviceWidgetReceiver : BreezoneWidgetReceiver(DeviceWidget())
class ScenesWidgetReceiver : BreezoneWidgetReceiver(ScenesWidget())

/** The widget of a kind, and the kind of a placed widget (by its receiver). */
internal fun widgetOf(kind: WidgetKind): BreezoneWidget = when (kind) {
    WidgetKind.Zone -> ZoneWidget()
    WidgetKind.Energy -> EnergyWidget()
    WidgetKind.Price -> PriceWidget()
    WidgetKind.Device -> DeviceWidget()
    WidgetKind.Scenes -> ScenesWidget()
}

internal fun kindOf(context: Context, appWidgetId: Int): WidgetKind? = when (AppWidgetManager.getInstance(context).getAppWidgetInfo(appWidgetId)?.provider?.className) {
    ZoneWidgetReceiver::class.java.name -> WidgetKind.Zone
    EnergyWidgetReceiver::class.java.name -> WidgetKind.Energy
    PriceWidgetReceiver::class.java.name -> WidgetKind.Price
    DeviceWidgetReceiver::class.java.name -> WidgetKind.Device
    ScenesWidgetReceiver::class.java.name -> WidgetKind.Scenes
    else -> null
}

/** Shows the widgets again after the app changed something (a command, a sign-in). */
suspend fun refreshWidgets(context: Context) {
    WidgetKind.entries.forEach { k -> runCatching { widgetOf(k).updateAll(context) } }
}

private suspend fun BreezoneWidget.updateAll(context: Context) {
    GlanceAppWidgetManager(context).getGlanceIds(this::class.java).forEach { refresh(context, it) }
}
