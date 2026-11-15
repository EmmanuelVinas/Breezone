package com.breezone.app.data

import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertTrue

class DevicesTest {
    @Test
    fun decodesModuleDevicesAndTheirStates() {
        val json = """[
            {"id":7,"kind":"light","traits":["light@1"],"source":"hue","source_id":2,"installation_id":null,"reference":"3",
             "name":"Plafonnier","room_id":1,"role":null,"measures":{"device_id":null,"room_id":null},"position":0,"available":true,
             "state":{"on":true,"brightness":70,"color_temp":2900,"color_temp_min":2000,"color_temp_max":6500,"mode":"white","reachable":true}},
            {"id":9,"kind":"shutter","traits":["cover.position@1"],"source":"homekit","source_id":3,"installation_id":null,"reference":"2.8",
             "name":"Baie","room_id":null,"position":1,"available":true,"state":{"position":40,"moving":"closing","target":0}},
            {"id":1,"kind":"climate.zone","traits":["climate.zone@1"],"source":"airzone","installation_id":1,
             "reference":{"system_id":1,"zone_id":2},"name":"Salon","room_id":null,"position":0,"available":true,"state":null}
        ]"""
        val devices = ServerDevices(FzJson.decodeFromString<List<DeviceView>>(json), loaded = true)
        assertEquals(listOf(7L, 9L), devices.modules.map { it.id }, "the Airzone zones are not modules")
        val light = devices.withTrait(TraitIds.LIGHT).single().stateAs(LightState.serializer())!!
        assertEquals(70L, light.brightness)
        assertEquals(LightMode.White, light.mode)
        val cover = devices.withTrait(TraitIds.COVER).single().stateAs(CoverPositionState.serializer())!!
        assertEquals(CoverPositionMoving.Closing, cover.moving)
        assertEquals(0L, cover.target)
    }

    @Test
    fun theModulesGoWithThisServersInstallations() {
        val local = Installation(id = 1, name = "Maison", access = "local")
        val node = Installation(id = 2, name = "Atelier", access = "node")
        val server = ServerState(ServerAccount(id = "a", url = "https://x", label = "x", username = "u"), installations = listOf(local, node))
        assertTrue(server.modulesHere(local))
        assertFalse(server.modulesHere(node), "another server's installation")
        // A server reaching only other servers' installations still shows its own modules.
        assertTrue(server.copy(installations = listOf(node)).modulesHere(node))
    }
}
