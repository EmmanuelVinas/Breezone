package com.breezone.app.data

import kotlinx.serialization.EncodeDefault
import kotlinx.serialization.ExperimentalSerializationApi
import kotlinx.serialization.Serializable
import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonNull
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.booleanOrNull
import kotlinx.serialization.json.contentOrNull
import kotlinx.serialization.json.jsonPrimitive
import kotlinx.serialization.json.longOrNull
import kotlin.random.Random

/** The page of the server that keeps the home of the app, apart from the web's overview. */
const val MOBILE_PAGE = "mobile"

/** How much of a row a card takes on a phone: half (two side by side) or the whole width. */
enum class BlockSize(val wire: String) {
    Half("half"),
    Full("full"),
    ;

    companion object {
        /** The web's quarter is half a phone's row; anything else, the whole row. */
        fun of(wire: String) = if (wire == "quarter" || wire == "half") Half else Full
    }
}

/** A card of a dashboard: what it shows is set by its type and `config` (see `ui/home/Blocks.kt`). */
// The server wants every field: those left at their default are written all the same.
@OptIn(ExperimentalSerializationApi::class)
@Serializable
data class DashboardBlock(
    val id: String,
    val type: String,
    @EncodeDefault val size: String = "full",
    @EncodeDefault val config: JsonObject = JsonObject(emptyMap()),
) {
    val phoneSize get() = BlockSize.of(size)

    fun long(key: String): Long? = (config[key] as? JsonPrimitive)?.longOrNull
    fun string(key: String): String? = (config[key] as? JsonPrimitive)?.takeIf { it.isString }?.contentOrNull
    fun bool(key: String): Boolean? = (config[key] as? JsonPrimitive)?.booleanOrNull
    fun has(key: String) = key in config
    fun strings(key: String): List<String> = (config[key] as? JsonArray)?.mapNotNull { (it as? JsonPrimitive)?.contentOrNull }.orEmpty()
    fun longs(key: String): List<Long> = (config[key] as? JsonArray)?.mapNotNull { (it as? JsonPrimitive)?.longOrNull }.orEmpty()
}

/** A device with a history, and the fields it has one of. */
@Serializable
data class Charted(val deviceId: Long, val fields: List<String> = emptyList())

@OptIn(ExperimentalSerializationApi::class)
@Serializable
data class DashboardLayout(@EncodeDefault val version: Int = 1, @EncodeDefault val blocks: List<DashboardBlock> = emptyList())

/** A user's own layout, the installation's, or none yet (`generated`: the client composes it). */
@Serializable
data class DashboardView(
    val layout: DashboardLayout? = null,
    val source: String = "generated",
    val hasDefault: Boolean = false,
    val canSetDefault: Boolean = false,
)

fun newBlockId(): String = Random.nextLong().toULong().toString(36).take(8)

fun block(type: String, size: BlockSize = BlockSize.Full, config: Map<String, JsonElement> = emptyMap()) =
    DashboardBlock(newBlockId(), type, size.wire, JsonObject(config))

fun DashboardBlock.withConfig(key: String, value: JsonElement) = copy(config = JsonObject(config + (key to value)))

/** The kinds of cards the app knows; one of another kind (written by a newer app) is kept, not shown. */
object BlockTypes {
    const val PAGES = "pages"
    const val WEATHER = "weather"
    const val ENERGY = "energy"
    const val ENERGY_FLOW = "energy_flow"
    const val ENERGY_CURVE = "energy_curve"
    const val ENERGY_BREAKDOWN = "energy_breakdown"
    const val GROUP = "group"
    const val DEVICE = "device"
    const val CHART = "chart"
    const val SCENES = "scenes"
    const val ZONES = "zones"
    const val PRESENCE = "presence"
}

/** What the home has, to compose a first layout from. */
data class HomeFacts(
    val devices: List<DeviceView>,
    val rooms: List<Room>,
    val roles: EnergyRoles?,
    val hasZones: Boolean,
    val hasRoutines: Boolean,
)

/**
 * The home of the app as it comes, before anyone arranges it: shortcuts to the pages, what the
 * home draws and makes now, the weather, a card per room, then the scenes.
 */
fun generateMobileLayout(home: HomeFacts): DashboardLayout {
    val blocks = buildList {
        add(block(BlockTypes.PAGES))
        val roles = home.roles
        if (roles?.any == true) {
            if (roles.solar) add(block(BlockTypes.ENERGY, BlockSize.Half, mapOf("flow" to JsonPrimitive("solar"))))
            add(block(BlockTypes.ENERGY, BlockSize.Half, mapOf("flow" to JsonPrimitive("home"))))
        }
        add(block(BlockTypes.WEATHER))
        val shown = home.devices.filter { it.shownOnCards() }
        if (shown.none { it.source != "airzone" }) {
            // The climate alone: its zones as on their page, rather than lines by room.
            if (home.hasZones) add(block(BlockTypes.ZONES))
        } else {
            home.rooms.sortedBy { it.position }.forEach { r ->
                if (shown.any { it.roomId == r.id }) add(block(BlockTypes.GROUP, config = mapOf("room_id" to JsonPrimitive(r.id))))
            }
            if (shown.any { it.roomId == null }) add(block(BlockTypes.GROUP, config = mapOf("room_id" to JsonNull)))
        }
        if (home.hasRoutines) add(block(BlockTypes.SCENES))
    }
    // Ids that stay the same from one composition to the next: the lists key their cards on them.
    return DashboardLayout(blocks = blocks.mapIndexed { i, b -> b.copy(id = "g$i") })
}

/** A device worth a line on a card: not a remote, nor a panel of a solar inverter. */
fun DeviceView.shownOnCards() = !has(TraitIds.PANEL) && !traits.any { it.startsWith("button@") }

/** The Airzone zone a device is (its reference), to drive it as the climate screens do. */
fun DeviceView.zoneRef(): Pair<Long, Long>? {
    if (source != "airzone") return null
    val r = reference as? JsonObject ?: return null
    val system = r["system_id"]?.jsonPrimitive?.longOrNull ?: return null
    val zone = r["zone_id"]?.jsonPrimitive?.longOrNull ?: return null
    return system to zone
}

fun groupConfig(roomId: Long?) = mapOf("room_id" to (roomId?.let { JsonPrimitive(it) } ?: JsonNull))

fun typeConfig(type: String) = mapOf("type" to JsonPrimitive(type))
