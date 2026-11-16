package com.breezone.app.data

import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.jsonObject
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertTrue

class RoutinesTest {
    @Test
    fun aDeviceActionIsReadAndWrittenBackAsItWas() {
        val json = """{"id":3,"name":"Je pars","actions":[
            {"installation_id":1,"system_id":1,"zone_id":2,"on":false},
            {"device_id":7,"trait":"switch.relay","action":"set","params":{"on":false},"delay_minutes":30}]}"""
        val routine = FzJson.decodeFromString<Routine>(json)
        val (zone, device) = routine.actions
        assertFalse(zone.isDevice)
        assertEquals(2L, zone.zoneId)
        assertTrue(device.isDevice)

        val written = FzJson.encodeToJsonElement(RoutineInput.serializer(), RoutineInput(routine.name, true, routine.actions)).jsonObject
        val actions = FzJson.parseToJsonElement(written["actions"].toString()) as kotlinx.serialization.json.JsonArray
        assertEquals(
            setOf("device_id", "trait", "action", "params", "delay_minutes"),
            actions[1].jsonObject.keys,
        )
        assertEquals(JsonPrimitive(false), actions[1].jsonObject["params"]!!.jsonObject["on"])
        assertEquals(setOf("installation_id", "system_id", "zone_id", "on"), actions[0].jsonObject.keys)
    }

    @Test
    fun aPausedChargerIsPausedWhateverItsStatusSays() {
        assertEquals(ChargerShown.Paused, EvChargerState(status = EvChargerStatus.Charging, paused = true).shown)
        assertEquals(ChargerShown.Paused, EvChargerState(status = EvChargerStatus.Available, vehicle = true, paused = true).shown)
        assertEquals(ChargerShown.Available, EvChargerState(status = EvChargerStatus.Available, paused = true).shown)
        assertEquals(ChargerShown.Fault, EvChargerState(status = EvChargerStatus.Fault, paused = true).shown)
        assertEquals(ChargerShown.Charging, EvChargerState(status = EvChargerStatus.Charging).shown)
    }
}
