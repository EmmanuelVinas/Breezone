package com.breezone.app.data

import io.ktor.client.HttpClient
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Job
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.MutableSharedFlow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.SharedFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import kotlinx.serialization.builtins.ListSerializer
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.jsonArray
import kotlinx.serialization.json.jsonPrimitive
import kotlinx.serialization.json.long
import com.breezone.app.resources.Res
import com.breezone.app.resources.common_failure
import com.breezone.app.resources.hub_command_refused
import com.breezone.app.resources.hub_read_only
import com.breezone.app.resources.hub_zones_no_answer
import com.breezone.app.resources.hub_zones_switched_off
import com.breezone.app.resources.hub_zones_switched_on
import com.breezone.app.resources.installation_group_default
import org.jetbrains.compose.resources.getPluralString
import org.jetbrains.compose.resources.getString
import kotlin.random.Random

enum class Live { Connecting, Live, Offline }

/** Everything the app knows about one Breezone server. */
data class ServerState(
    val account: ServerAccount,
    val user: User? = null,
    val installations: List<Installation> = emptyList(),
    /** Installations of the Airzone account whose WebServer is on another network. */
    val remote: List<CloudInstallation> = emptyList(),
    val live: Live = Live.Connecting,
    val loaded: Boolean = false,
    /** The refresh token was refused or lost: the password must be typed again. */
    val authRequired: Boolean = false,
    /** The app and this server are too far apart in version: which one must be updated. */
    val update: UpdateNeeded? = null,
    /** Version of the server software, as announced by `/health`. */
    val serverVersion: String? = null,
    /** The server speaks a newer API: still compatible, an app update would bring its new features. */
    val appUpdateAvailable: Boolean = false,
    val error: String? = null,
)

private const val SERVERS_KEY = "servers"

/**
 * Central repository of the app. Holds one connection per Breezone server, keeps its state up
 * to date through the WebSocket push channel and applies commands optimistically.
 */
class Hub(
    private val http: HttpClient,
    private val store: SecureStore,
    private val scope: CoroutineScope,
) {
    private val _servers = MutableStateFlow<List<ServerState>>(emptyList())
    val servers: StateFlow<List<ServerState>> = _servers.asStateFlow()

    private val _messages = MutableSharedFlow<String>(extraBufferCapacity = 8)
    /** User-facing messages (errors, confirmations) for snackbars. */
    val messages: SharedFlow<String> = _messages

    private val apis = mutableMapOf<String, BreezoneApi>()
    /** Answers the requests of the built-in demonstration server (see `Demo.kt`). */
    private val demoHttp by lazy { demoHttpClient() }
    private fun httpFor(account: ServerAccount) = if (isDemo(account.url)) demoHttp else http
    private val jobs = mutableMapOf<String, Job>()
    val discovery = BreezoneDiscovery(http)

    init {
        val saved = store.get(SERVERS_KEY)?.let {
            runCatching { FzJson.decodeFromString(ListSerializer(ServerAccount.serializer()), it) }.getOrNull()
        } ?: emptyList()
        saved.forEach { a -> a.certificate?.let { TlsTrust.pin(a.url, it) } }
        _servers.value = saved.map { ServerState(it, authRequired = it.refreshToken == null) }
        saved.filter { it.refreshToken != null }.forEach { start(it) }
    }

    fun api(serverId: String): BreezoneApi? = apis[serverId]

    fun server(serverId: String): ServerState? = _servers.value.firstOrNull { it.account.id == serverId }

    // -------------------------------------------------------------------------------------
    // Accounts
    // -------------------------------------------------------------------------------------

    private fun persist() {
        val accounts = _servers.value.map { it.account }
        store.set(SERVERS_KEY, FzJson.encodeToString(ListSerializer(ServerAccount.serializer()), accounts))
    }

    private fun updateServer(id: String, transform: (ServerState) -> ServerState) {
        _servers.update { list -> list.map { if (it.account.id == id) transform(it) else it } }
    }

    private fun updateAccount(id: String, transform: (ServerAccount) -> ServerAccount) {
        updateServer(id) { it.copy(account = transform(it.account)) }
        persist()
    }

    /**
     * Signs in to a server (or creates its first administrator when `setup`) and starts
     * following it. Re-signing to an already known server replaces its session.
     */
    suspend fun signIn(url: String, username: String, password: String, setup: Boolean = false, certificate: String? = null): ServerAccount {
        val base = normalizeUrl(url)
        val pair = discovery.login(base, username, password, setup)
        val existing = _servers.value.firstOrNull { normalizeUrl(it.account.url) == base && it.account.username.equals(username.trim(), true) }
        val label = base.removePrefix("https://").removePrefix("http://").substringBefore('/')
        val account = ServerAccount(
            id = existing?.account?.id ?: Random.nextLong().toULong().toString(16),
            url = base,
            label = existing?.account?.label ?: label,
            username = pair.user.username,
            refreshToken = pair.refreshToken,
            certificate = certificate ?: existing?.account?.certificate,
        )
        jobs.remove(account.id)?.cancel()
        _servers.update { list ->
            if (existing != null) list.map { if (it.account.id == account.id) ServerState(account, user = pair.user) else it }
            else list + ServerState(account, user = pair.user)
        }
        persist()
        start(account, pair)
        return account
    }

    /**
     * Adds the built-in demonstration server (simulated installations, nothing leaves the
     * phone): to try the app without a server, and for store reviews.
     */
    suspend fun startDemo(label: String): ServerAccount {
        _servers.value.firstOrNull { isDemo(it.account.url) }?.let { return it.account }
        val pair = BreezoneDiscovery(demoHttp).login(DEMO_URL, "demo", "demo")
        val account = ServerAccount(id = "demo", url = DEMO_URL, label = label, username = pair.user.username, refreshToken = pair.refreshToken)
        _servers.update { it + ServerState(account, user = pair.user) }
        persist()
        start(account, pair)
        return account
    }

    /** Signs out of a server and forgets it. */
    fun removeServer(id: String) {
        val api = apis.remove(id)
        jobs.remove(id)?.cancel()
        _servers.update { list -> list.filterNot { it.account.id == id } }
        persist()
        scope.launch { api?.logout() }
    }

    fun rename(id: String, label: String) {
        if (label.isNotBlank()) updateAccount(id) { it.copy(label = label.trim()) }
    }

    // -------------------------------------------------------------------------------------
    // Live state
    // -------------------------------------------------------------------------------------

    private fun start(account: ServerAccount, pair: TokenPair? = null) {
        val api = BreezoneApi(httpFor(account), account) { rotated -> updateAccount(account.id) { it.copy(refreshToken = rotated) } }
        pair?.let(api::seed)
        apis[account.id] = api
        jobs[account.id] = scope.launch { follow(account.id, api) }
    }

    private suspend fun follow(id: String, api: BreezoneApi) {
        var attempt = 0
        while (true) {
            try {
                updateServer(id) { it.copy(live = Live.Connecting) }
                val health = api.health()
                updateServer(id) { it.copy(serverVersion = health.version, appUpdateAvailable = appUpdateAvailable(health)) }
                checkCompatible(health)
                val user = api.user ?: api.me()
                val installations = api.status()
                val remote = runCatching { api.cloud().installations.filter { !it.reachable } }.getOrDefault(emptyList())
                updateServer(id) { it.copy(user = user, installations = installations, remote = remote, loaded = true, error = null, authRequired = false, update = null) }
                api.live().collect { msg ->
                    attempt = 0
                    handle(id, msg)
                }
            } catch (e: CancellationException) {
                throw e
            } catch (e: IncompatibleException) {
                // Retrying will not help: wait for an update (checked again when the app comes back).
                updateServer(id) { it.copy(update = e.update, live = Live.Offline, error = e.message) }
                return
            } catch (e: AuthRequiredException) {
                updateAccount(id) { it.copy(refreshToken = null) }
                updateServer(id) { it.copy(authRequired = true, live = Live.Offline) }
                return
            } catch (e: Exception) {
                val message = explain(e, api.account.url) ?: e.message
                updateServer(id) { it.copy(live = Live.Offline, error = message) }
            }
            updateServer(id) { it.copy(live = Live.Offline) }
            delay(minOf(30_000L, 1_000L shl minOf(attempt++, 5)))
        }
    }

    private fun handle(id: String, msg: JsonObject) {
        when (msg["type"]?.jsonPrimitive?.content) {
            "hello" -> updateServer(id) { it.copy(live = Live.Live) }
            "snapshot" -> {
                val list = FzJson.decodeFromJsonElement(ListSerializer(Installation.serializer()), msg["installations"]!!.jsonArray)
                updateServer(id) { it.copy(installations = list, live = Live.Live, loaded = true) }
            }
            "installation" -> {
                val inst = FzJson.decodeFromJsonElement(Installation.serializer(), msg["installation"]!!)
                updateServer(id) { s ->
                    val exists = s.installations.any { it.id == inst.id }
                    s.copy(installations = if (exists) s.installations.map { if (it.id == inst.id) inst else it } else (s.installations + inst).sortedBy { it.id })
                }
            }
            "installation_removed" -> {
                val removed = msg["id"]!!.jsonPrimitive.long
                updateServer(id) { s -> s.copy(installations = s.installations.filterNot { it.id == removed }) }
            }
            "error" -> updateServer(id) { it.copy(live = Live.Offline) }
        }
    }

    /** Reconnects every server (e.g. when the app comes back to the foreground). */
    fun refresh() {
        _servers.value.forEach { s ->
            val api = apis[s.account.id] ?: return@forEach
            if (s.live != Live.Live && !s.authRequired) {
                jobs.remove(s.account.id)?.cancel()
                jobs[s.account.id] = scope.launch { follow(s.account.id, api) }
            }
        }
    }

    // -------------------------------------------------------------------------------------
    // Commands (optimistic)
    // -------------------------------------------------------------------------------------

    private fun applyLocally(serverId: String, zone: Zone, cmd: ZoneCommand) {
        updateServer(serverId) { s ->
            s.copy(installations = s.installations.map { inst ->
                if (inst.id != zone.installationId) inst
                else inst.copy(zones = inst.zones.map { z ->
                    when {
                        z.sameAs(zone) -> z.copy(
                            on = cmd.on ?: z.on,
                            setpoint = cmd.setpoint ?: z.setpoint,
                            mode = cmd.mode ?: z.mode,
                            speed = cmd.speed ?: z.speed,
                            sleep = cmd.sleep ?: z.sleep,
                        )
                        // The operation mode is system-wide.
                        cmd.mode != null && z.systemId == zone.systemId -> z.copy(mode = cmd.mode)
                        else -> z
                    }
                })
            })
        }
    }

    private suspend fun resync(serverId: String) {
        val api = apis[serverId] ?: return
        runCatching { api.status() }.onSuccess { list -> updateServer(serverId) { it.copy(installations = list) } }
    }

    /** Installation this user can only look at (read-only access, or shared read-only by its node): commands are not sent. */
    private fun readOnly(serverId: String, zones: List<Zone>): Installation? =
        server(serverId)?.installations?.firstOrNull { inst -> !inst.controllable && zones.any { it.installationId == inst.id } }

    // -----------------------------------------------------------------------------------
    // Visual acknowledgement of commands: pending → done ✓ (or error) on the zone card.
    // -----------------------------------------------------------------------------------

    private val _commandStatus = MutableStateFlow<Map<String, CommandStatus>>(emptyMap())
    val commandStatus: StateFlow<Map<String, CommandStatus>> = _commandStatus.asStateFlow()
    private val statusJobs = mutableMapOf<String, Job>()

    private fun setStatus(serverId: String, zones: List<Zone>, status: CommandStatus) {
        val keys = zones.map { commandKey(serverId, it) }
        keys.forEach { statusJobs.remove(it)?.cancel() }
        _commandStatus.update { it + keys.associateWith { status } }
        if (status == CommandStatus.Pending) return
        keys.forEach { key ->
            statusJobs[key] = scope.launch {
                delay(if (status == CommandStatus.Done) 1_500 else 3_000)
                _commandStatus.update { it - key }
            }
        }
    }

    /**
     * Sends a command to one zone. [successMessage] is shown once the server confirmed it (for
     * actions whose effect is not visible on the card, e.g. a system-wide mode).
     */
    fun control(serverId: String, zone: Zone, cmd: ZoneCommand, successMessage: (suspend () -> String)? = null) {
        val api = apis[serverId] ?: return
        readOnly(serverId, listOf(zone))?.let { inst ->
            scope.launch { _messages.tryEmit(getString(Res.string.hub_read_only, inst.name)) }
            return
        }
        applyLocally(serverId, zone, cmd)
        setStatus(serverId, listOf(zone), CommandStatus.Pending)
        scope.launch {
            try {
                api.control(zone, cmd)
                setStatus(serverId, listOf(zone), CommandStatus.Done)
                successMessage?.let { _messages.tryEmit(it()) }
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                setStatus(serverId, listOf(zone), CommandStatus.Error)
                _messages.tryEmit(getString(Res.string.hub_command_refused, e.message.orEmpty()))
                resync(serverId)
            }
        }
    }

    /** Switches several zones (a group) on or off, then confirms with a message. */
    fun setPower(serverId: String, zones: List<Zone>, on: Boolean, label: String? = null) {
        val api = apis[serverId] ?: return
        readOnly(serverId, zones)?.let { inst ->
            scope.launch { _messages.tryEmit(getString(Res.string.hub_read_only, inst.name)) }
            return
        }
        zones.forEach { applyLocally(serverId, it, ZoneCommand(on = on)) }
        setStatus(serverId, zones, CommandStatus.Pending)
        scope.launch {
            try {
                val result = api.batch(zones.map { Action(it.installationId, it.systemId, it.zoneId, on = on) })
                zones.forEachIndexed { i, z ->
                    setStatus(serverId, listOf(z), if (result.results.getOrNull(i)?.ok == false) CommandStatus.Error else CommandStatus.Done)
                }
                val failed = result.results.count { !it.ok }
                val ok = zones.size - failed
                if (failed > 0) {
                    _messages.tryEmit(getPluralString(Res.plurals.hub_zones_no_answer, failed, failed))
                } else {
                    val group = label ?: getString(Res.string.installation_group_default)
                    val text = if (on) Res.plurals.hub_zones_switched_on else Res.plurals.hub_zones_switched_off
                    _messages.tryEmit(getPluralString(text, ok, group, ok))
                }
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                setStatus(serverId, zones, CommandStatus.Error)
                _messages.tryEmit(getString(Res.string.common_failure, e.message.orEmpty()))
                resync(serverId)
            }
        }
    }

    fun notify(message: String) {
        _messages.tryEmit(message)
    }
}

/** Acknowledgement of a command sent to a zone. */
enum class CommandStatus { Pending, Done, Error }

/** Key of a zone in [Hub.commandStatus]. */
fun commandKey(serverId: String, zone: Zone): String = "$serverId/${zone.installationId}/${zone.systemId}/${zone.zoneId}"
