package com.breezone.app.data

import kotlinx.serialization.KSerializer
import kotlinx.serialization.Serializable
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonNull
import kotlinx.serialization.json.JsonObject

/** A device of a server (`/api/devices`): an Airzone zone or a device of a module. */
@Serializable
data class DeviceView(
    val id: Long,
    val kind: String = "",
    val traits: List<String> = emptyList(),
    val source: String = "",
    val sourceId: Long? = null,
    val installationId: Long? = null,
    val reference: JsonElement = JsonNull,
    val name: String = "",
    val roomId: Long? = null,
    val position: Long = 0,
    val available: Boolean = true,
    val state: JsonObject? = null,
) {
    fun has(traitId: String) = traitId in traits

    /** The state as a trait describes it (its defaults when unknown). */
    fun <T> stateAs(serializer: KSerializer<T>): T? =
        state?.let { runCatching { FzJson.decodeFromJsonElement(serializer, it) }.getOrNull() }
}

@Serializable
data class Room(
    val id: Long,
    val name: String,
    val position: Long = 0,
)

/** A scene of a module's own app (Hue). */
@Serializable
data class SceneView(
    val sourceId: Long,
    val id: String,
    val name: String,
    val room: String? = null,
    val deviceIds: List<Long> = emptyList(),
)

@Serializable
data class DeviceActionInput(
    val trait: String,
    val action: String,
    val params: JsonObject,
)

/** What a server's modules provide (lights, shutters, ventilation…), read on demand. */
data class ServerDevices(
    val devices: List<DeviceView> = emptyList(),
    val rooms: List<Room> = emptyList(),
    val scenes: List<SceneView> = emptyList(),
    val loaded: Boolean = false,
) {
    /** The devices of modules (not the Airzone zones, shown by the climate screens). */
    val modules: List<DeviceView> get() = devices.filter { it.sourceId != null }
    fun withTrait(traitId: String) = modules.filter { it.has(traitId) }
}

object TraitIds {
    const val LIGHT = "light@1"
    const val RELAY = "switch.relay@1"
    const val COVER = "cover.position@1"
    const val VENTILATION_UNIT = "ventilation.unit@1"
    const val VENTILATION_ROOM = "ventilation.room@1"
}

/**
 * The modules are a server's own: they go with its installations, not with one reached through
 * another Breezone server (unless the server has no installation of its own).
 */
fun ServerState.modulesHere(installation: Installation?): Boolean =
    installation?.access != "node" || installations.none { it.access == "local" }

/** The history of a device (`/api/devices/{id}/history`): `[ts, value]` points by field. */
@Serializable
data class DeviceHistory(
    val bucket: Long = 0,
    val series: Map<String, List<List<Double>>> = emptyMap(),
)
