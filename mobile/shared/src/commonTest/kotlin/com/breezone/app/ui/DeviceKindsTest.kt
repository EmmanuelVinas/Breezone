package com.breezone.app.ui

import com.breezone.app.data.DeviceView
import com.breezone.app.data.KIND_IDS
import com.breezone.app.ui.home.DeviceKind
import com.breezone.app.ui.home.deviceKind
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertTrue

class DeviceKindsTest {
    private fun kind(kind: String, vararg traits: String) = DeviceView(id = 1, kind = kind, traits = traits.toList()).deviceKind

    @Test
    fun theKindComesFromTheCatalog() {
        assertEquals(DeviceKind.Window, kind("window", "cover.position@1"))
        assertEquals(DeviceKind.Shutter, kind("cover.position", "cover.position@1"))
        assertEquals(DeviceKind.Light, kind("light", "light@1", "energy.power@1"))
        assertEquals(DeviceKind.Energy, kind("energy.power", "energy.power@1", "energy.meter@1"))
        assertEquals(DeviceKind.Switch, kind("switch.relay", "switch.relay@1", "energy.power@1"))
        assertEquals(DeviceKind.Sensor, kind("sensor", "environment.sensor@1"))
        assertEquals(DeviceKind.RoomAir, kind("ventilation.room", "ventilation.room@1"))
        assertEquals(DeviceKind.Other, kind("pool.filtration", "pool.filtration@1"), "a trait this app does not know")
    }

    @Test
    fun theKindsAreThoseOfTheCatalog() {
        val here = DeviceKind.entries.map { it.id }
        assertTrue(KIND_IDS.containsAll(here), "${here - KIND_IDS.toSet()} are not kinds of contracts/ui.json")
        assertEquals(here, KIND_IDS.filter { it in here }, "same order as contracts/ui.json")
    }
}
