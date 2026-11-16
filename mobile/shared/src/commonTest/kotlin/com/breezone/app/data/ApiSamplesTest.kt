package com.breezone.app.data

import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertTrue

/** The app reads every view of the server's contract (`server/contracts/views.json`). */
class ApiSamplesTest {
    @Test
    fun readsEveryViewOfTheContract() {
        val installation = FzJson.decodeFromString(Installation.serializer(), ApiSamples.INSTALLATION)
        assertEquals("x", installation.name)
        val zone = installation.zones.single()
        assertEquals(Mode.Stop, zone.mode)
        assertTrue(zone.airQuality != null)
        FzJson.decodeFromString(Zone.serializer(), ApiSamples.ZONE)
        FzJson.decodeFromString(Connection.serializer(), ApiSamples.CONNECTION)
        FzJson.decodeFromString(AirQuality.serializer(), ApiSamples.AIR_QUALITY)
        FzJson.decodeFromString(NodeRef.serializer(), ApiSamples.NODE_REF)
        FzJson.decodeFromString(InstallationUserRights.serializer(), ApiSamples.INSTALLATION_USER_RIGHTS)
        val device = FzJson.decodeFromString(DeviceView.serializer(), ApiSamples.DEVICE_VIEW)
        assertEquals(true, device.canControl)
        assertEquals("x", FzJson.decodeFromString(Room.serializer(), ApiSamples.ROOM).name)
    }
}
