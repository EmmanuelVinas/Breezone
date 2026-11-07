package com.breezone.app.data

import io.ktor.client.HttpClient
import io.ktor.client.engine.darwin.Darwin
import io.ktor.client.plugins.contentnegotiation.ContentNegotiation
import io.ktor.client.plugins.websocket.WebSockets
import io.ktor.serialization.kotlinx.json.json
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.test.runTest
import kotlinx.coroutines.withContext
import kotlinx.coroutines.withTimeout
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertTrue
import kotlin.time.Duration.Companion.seconds

/**
 * End-to-end check of the mobile data layer against a Breezone **development** server:
 * `BREEZONE_DEV_PORT=8091 BREEZONE_ADMIN_USER=admin BREEZONE_ADMIN_PASSWORD=motdepasse1 ./scripts/dev.sh --reset`.
 * It uses a dedicated port so that it never touches a real instance (usually on :8427), and is
 * skipped when no server answers there or when the test credentials are refused.
 */
class LiveServerTest {
    private val url = "http://localhost:8091"

    @Test
    fun signInFollowAndControl() = runTest(timeout = 60.seconds) {
        withContext(Dispatchers.Default) {
            val http = HttpClient(Darwin) {
                install(ContentNegotiation) { json(FzJson) }
                install(WebSockets)
            }
            if (runCatching { BreezoneDiscovery(http).probe(url) }.isFailure) {
                println("LiveServerTest skipped: no Breezone server on $url")
                return@withContext
            }
            val scope = CoroutineScope(SupervisorJob() + Dispatchers.Default)
            try {
                val hub = Hub(http, SecureStore(), scope)
                val account = try {
                    hub.signIn(url, "admin", "motdepasse1")
                } catch (e: ApiException) {
                    println("LiveServerTest skipped: test credentials refused by $url (${e.message})")
                    return@withContext
                }
                val live = withTimeout(20.seconds) {
                    hub.servers.first { list -> list.any { it.account.id == account.id && it.live == Live.Live && it.installations.isNotEmpty() } }
                }.first { it.account.id == account.id }
                val zone = live.installations.first().zones.first { it.name == "Bureau" }

                // Optimistic update, then the server echoes the new state over the WebSocket.
                val target = if (zone.setpoint == 21.5) 20.5 else 21.5
                hub.control(account.id, zone, ZoneCommand(setpoint = target, on = true))
                val updated = withTimeout(10.seconds) {
                    hub.servers.first { list ->
                        list.first { it.account.id == account.id }.installations.first().zones.first { it.sameAs(zone) }.setpoint == target
                    }
                }
                assertTrue(updated.isNotEmpty())
                // The command is sent asynchronously: wait until the server confirms it.
                val fresh = withTimeout(10.seconds) {
                    var z: Zone
                    do {
                        z = hub.api(account.id)!!.status().first().zones.first { it.sameAs(zone) }
                        if (z.setpoint != target) delay(200)
                    } while (z.setpoint != target)
                    z
                }
                assertEquals(target, fresh.setpoint)
                assertTrue(fresh.on)

                hub.removeServer(account.id)
                assertTrue(hub.servers.value.none { it.account.id == account.id })
            } finally {
                scope.cancel()
                http.close()
            }
        }
    }
}
