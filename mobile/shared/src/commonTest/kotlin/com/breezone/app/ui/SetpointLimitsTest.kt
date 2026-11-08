package com.breezone.app.ui

import com.breezone.app.data.Zone
import kotlin.test.Test
import kotlin.test.assertEquals

class SetpointLimitsTest {
    private fun zone(min: Double? = null, max: Double? = null, step: Double = 0.5) =
        Zone(installationId = 1, systemId = 1, zoneId = 1, name = "Salon", minTemp = min, maxTemp = max, step = step)

    @Test
    fun defaultsWhenZonesSayNothing() {
        assertEquals(SetpointLimits(15.0, 30.0, 0.5), SetpointLimits.of(emptyList()))
        assertEquals(SetpointLimits(15.0, 30.0, 0.5), SetpointLimits.of(listOf(zone())))
    }

    @Test
    fun keepsWhatEveryZoneAccepts() {
        val limits = SetpointLimits.of(listOf(zone(min = 16.0, max = 28.0), zone(min = 18.0, max = 30.0, step = 1.0)))
        assertEquals(SetpointLimits(18.0, 28.0, 1.0), limits)
        assertEquals(18.0, limits.clamp(12.0))
        assertEquals(22.0, limits.clamp(21.6))
    }

    @Test
    fun fallsBackWhenZonesDisagree() {
        assertEquals(SetpointLimits(15.0, 30.0, 0.5), SetpointLimits.of(listOf(zone(min = 25.0), zone(max = 20.0))))
    }
}
