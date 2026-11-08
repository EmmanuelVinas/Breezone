package com.breezone.app.data

import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertNull
import kotlin.test.assertTrue

class ApiVersionTest {
    @Test
    fun serversWithoutApiVersionSpeakApi1() {
        val health = FzJson.decodeFromString(Health.serializer(), """{"status":"ok","version":"0.4.1"}""")
        assertEquals(1, health.apiVersion)
        assertNull(updateNeeded(health))
    }

    @Test
    fun tooOldServerOrAppIsReported() {
        assertEquals(UpdateNeeded.Server, updateNeeded(Health("ok", "0.1.0", apiVersion = MIN_SERVER_API - 1)))
        assertEquals(UpdateNeeded.App, updateNeeded(Health("ok", "9.0.0", apiVersion = CLIENT_API + 1, minClientApi = CLIENT_API + 1)))
    }

    @Test
    fun newerCompatibleServerSuggestsAnAppUpdate() {
        assertTrue(appUpdateAvailable(Health("ok", "1.0.0", apiVersion = CLIENT_API + 1, minClientApi = CLIENT_API)))
        assertFalse(appUpdateAvailable(Health("ok", "0.5.0", apiVersion = CLIENT_API)))
    }
}
