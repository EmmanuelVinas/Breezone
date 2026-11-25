package com.breezone.app.widgets

import android.content.Context
import com.breezone.app.data.BreezoneApi
import com.breezone.app.data.DeviceView
import com.breezone.app.data.FzJson
import com.breezone.app.data.Hub
import kotlinx.serialization.Serializable
import org.koin.core.context.GlobalContext

/** The kinds of widgets of the home screen. */
enum class WidgetKind { Zone, Energy, Price, Device, Scenes }

/** What a widget shows, chosen when it is placed. */
@Serializable
data class WidgetConfig(
    val serverId: String,
    val installationId: Long? = null,
    val systemId: Long? = null,
    val zoneId: Long? = null,
    val deviceId: Long? = null,
    /** The lights of a room, switched together. */
    val roomId: Long? = null,
    /** All the lights of the home, switched together. */
    val allLights: Boolean = false,
    /** Shown while the server cannot be read. */
    val title: String = "",
)

/** The choices of the widgets, kept by widget id. */
object WidgetStore {
    private fun prefs(context: Context) = context.getSharedPreferences("breezone_widgets", Context.MODE_PRIVATE)

    fun get(context: Context, appWidgetId: Int): WidgetConfig? =
        prefs(context).getString("w$appWidgetId", null)?.let { runCatching { FzJson.decodeFromString(WidgetConfig.serializer(), it) }.getOrNull() }

    fun set(context: Context, appWidgetId: Int, config: WidgetConfig) {
        prefs(context).edit().putString("w$appWidgetId", FzJson.encodeToString(WidgetConfig.serializer(), config)).apply()
    }

    fun remove(context: Context, appWidgetId: Int) {
        prefs(context).edit().remove("w$appWidgetId").apply()
    }
}

/**
 * The hub of the app: the widgets go through its sessions (a refresh token is used once; a second
 * client of its own would sign the user out).
 */
internal fun hub(): Hub = GlobalContext.get().get()

internal fun api(serverId: String): BreezoneApi? = hub().api(serverId)

/** A device a widget can switch: a light or a plug (on/off), or a charger (shown). */
internal fun DeviceView.widgetTrait(): String? = listOf("light", "switch.relay", "ev.charger").firstOrNull { t -> traits.any { it.startsWith("$t@") } }

/** The widget switches lights together (a room's, or all of them) instead of one device. */
internal val WidgetConfig.lightGroup get() = allLights || roomId != null

/** The lights of a widget of a room, or of the whole home. */
internal fun List<DeviceView>.lightsOf(config: WidgetConfig): List<DeviceView> =
    filter { d -> d.sourceId != null && d.traits.any { it.startsWith("light@") } && (config.allLights || d.roomId == config.roomId) }.sortedBy { it.position }

/** Lights switched together, and the name of their room. */
internal data class LightGroup(val name: String, val lights: List<DeviceView>)
