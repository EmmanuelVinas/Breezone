package com.breezone.app.data

import kotlinx.serialization.ExperimentalSerializationApi
import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonNamingStrategy

/** JSON settings matching the Breezone server API (snake_case, tolerant to new fields). */
@OptIn(ExperimentalSerializationApi::class)
val FzJson = Json {
    ignoreUnknownKeys = true
    explicitNulls = false
    namingStrategy = JsonNamingStrategy.SnakeCase
    coerceInputValues = true
}

@Serializable
enum class Mode {
    @SerialName("stop") Stop,
    @SerialName("cooling") Cooling,
    @SerialName("heating") Heating,
    @SerialName("fan") Fan,
    @SerialName("dry") Dry,
    @SerialName("aux_heating") AuxHeating,
    @SerialName("auto") Auto,
}

@Serializable
data class User(val id: Long, val username: String, val role: String) {
    val isAdmin get() = role == "admin"
}

@Serializable
data class TokenPair(
    val accessToken: String,
    val refreshToken: String,
    val expiresIn: Long,
    val user: User,
)

@Serializable
data class Connection(
    val state: String = "unknown",
    val lastSeen: Long? = null,
    val lastError: String? = null,
    val latencyMs: Long? = null,
) {
    val online get() = state == "online"
}

@Serializable
data class Zone(
    val installationId: Long,
    val systemId: Long,
    val zoneId: Long,
    val name: String,
    val airzoneName: String? = null,
    val group: String? = null,
    val position: Long = 0,
    val on: Boolean = false,
    val roomTemp: Double? = null,
    val humidity: Long? = null,
    val setpoint: Double? = null,
    val minTemp: Double? = null,
    val maxTemp: Double? = null,
    val step: Double = 0.5,
    val units: String = "celsius",
    val mode: Mode? = null,
    val modes: List<Mode> = emptyList(),
    val isMaster: Boolean = false,
    val speed: Long? = null,
    val speeds: List<Long> = emptyList(),
    val sleep: Long? = null,
    val demand: Boolean = false,
    val batteryLow: Boolean = false,
    val errors: List<String> = emptyList(),
) {
    fun sameAs(other: Zone) = installationId == other.installationId && systemId == other.systemId && zoneId == other.zoneId
}

@Serializable
data class Installation(
    val id: Long,
    val name: String,
    val location: String? = null,
    val host: String = "",
    val port: Long = 3000,
    val enabled: Boolean = true,
    /** "local" (WebServer on the server's network) or "node" (through another Breezone server). */
    val access: String = "local",
    val node: NodeRef? = null,
    /** False when the other Breezone server shares it read-only. */
    val controllable: Boolean = true,
    val connection: Connection = Connection(),
    val zones: List<Zone> = emptyList(),
)

/** Breezone server an installation is reached through (ADR 0008). */
@Serializable
data class NodeRef(
    val id: Long,
    val name: String,
    /** online | offline | revoked | expired | error | unknown */
    val state: String = "unknown",
)

/** A command on a zone: only non-null fields are sent. */
@Serializable
data class ZoneCommand(
    val on: Boolean? = null,
    val setpoint: Double? = null,
    val mode: Mode? = null,
    val speed: Long? = null,
    val sleep: Long? = null,
)

@Serializable
data class Action(
    val installationId: Long,
    val systemId: Long,
    val zoneId: Long,
    val on: Boolean? = null,
    val setpoint: Double? = null,
    val mode: Mode? = null,
    /** Run that many minutes after the routine starts (kept when editing elsewhere). */
    val delayMinutes: Long? = null,
)

@Serializable
data class Routine(
    val id: Long,
    val name: String,
    val enabled: Boolean = true,
    val actions: List<Action> = emptyList(),
    val lastRunAt: Long? = null,
)

@Serializable
data class RoutineInput(
    val name: String,
    val enabled: Boolean,
    val actions: List<Action>,
)

@Serializable
data class Reading(
    val ts: Long,
    val roomTemp: Double? = null,
    val humidity: Long? = null,
    val setpoint: Double? = null,
    val isOn: Boolean? = null,
)

@Serializable
data class CloudInstallation(
    val cloudId: String,
    val name: String,
    val location: String? = null,
    val zones: Long = 0,
    val installationId: Long? = null,
    val reachable: Boolean = false,
)

@Serializable
data class CloudStatus(
    val autoSync: Boolean = false,
    val email: String? = null,
    val lastSync: Long? = null,
    val lastError: String? = null,
    val installations: List<CloudInstallation> = emptyList(),
)

@Serializable
data class SyncReport(
    val created: List<String> = emptyList(),
    val updated: List<String> = emptyList(),
    val unreachable: List<String> = emptyList(),
    /** Why some installations were not reached (from the server, already localized). */
    val notes: List<String> = emptyList(),
)

@Serializable
data class CloudResult(val report: SyncReport, val status: CloudStatus)

@Serializable
data class ActionResult(val ok: Boolean, val error: String? = null)

@Serializable
data class RunResult(val results: List<ActionResult> = emptyList())

@Serializable
data class SetupStatus(val needsSetup: Boolean)

/** A Breezone server the app is connected to. Persisted in the secure store. */
@Serializable
data class ServerAccount(
    val id: String,
    val url: String,
    val label: String,
    val username: String,
    val refreshToken: String? = null,
    /** HTTPS certificate trusted although it fails the usual checks (SHA-256), if any. */
    val certificate: String? = null,
)

/** Zone targeted by an Airzone schedule (the installation is the schedule's). */
@Serializable
data class ScheduleZone(val systemId: Long, val zoneId: Long)

/** Second step of some Airzone schedules ("on at 7:00, off at 9:00"): shown, edited in the Airzone app. */
@Serializable
data class ScheduleEnd(val time: String, val action: ZoneCommand = ZoneCommand())

/** A schedule of the Airzone app, mirrored from Airzone Cloud (Airzone runs it). */
@Serializable
data class AirzoneSchedule(
    val cloudId: String,
    val installation: String = "",
    val installationId: Long? = null,
    val id: String,
    val name: String,
    val enabled: Boolean = true,
    val kind: String = "",
    /** ISO days, 1 = Monday. */
    val days: List<Int> = emptyList(),
    val time: String? = null,
    val zones: List<ScheduleZone> = emptyList(),
    val action: ZoneCommand = ZoneCommand(),
    val end: ScheduleEnd? = null,
    /** False for the kinds Breezone cannot rewrite faithfully: edit them in the Airzone app. */
    val editable: Boolean = true,
    val note: String? = null,
)

@Serializable
data class SchedulesState(
    val lastSync: Long? = null,
    val lastError: String? = null,
    /** True when the server remembers an Airzone account (needed to read and edit schedules). */
    val account: Boolean = false,
    val schedules: List<AirzoneSchedule> = emptyList(),
)

@Serializable
data class AirzoneScheduleInput(
    val cloudId: String? = null,
    val name: String,
    val enabled: Boolean,
    val days: List<Int>,
    val time: String,
    val zones: List<ScheduleZone>,
    val action: ZoneCommand,
)

/** The zone [zoneId] of system [systemId] in this installation. */
fun Installation.zone(systemId: Long, zoneId: Long): Zone? = zones.firstOrNull { it.systemId == systemId && it.zoneId == zoneId }
