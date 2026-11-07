package com.breezone.app.data

import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertTrue

class ModelsTest {
    @Test
    fun decodesServerInstallation() {
        val json = """{"id":1,"name":"Maison","location":"Annecy","host":"127.0.0.1","port":3000,"enabled":true,
            "connection":{"state":"online","last_seen":1,"last_error":null,"latency_ms":3},
            "zones":[{"installation_id":1,"system_id":1,"zone_id":3,"name":"Chambre 1","airzone_name":"Chambre 1","group":"Etage",
            "position":2,"on":true,"room_temp":18.4,"humidity":45,"setpoint":17.5,"min_temp":15.0,"max_temp":30.0,"step":0.5,
            "units":"celsius","mode":"heating","modes":["stop","cooling","heating","fan","dry"],"is_master":false,"speed":0,
            "speeds":[0,1,2,3],"sleep":0,"double_setpoint":false,"demand":true,"battery_low":false,"errors":[],"new_field":"ignored"}]}"""
        val inst = FzJson.decodeFromString<Installation>(json)
        assertTrue(inst.connection.online)
        val z = inst.zones.single()
        assertEquals(Mode.Heating, z.mode)
        assertEquals("Etage", z.group)
        assertEquals(5, z.modes.size)
        assertFalse(z.isMaster)
        assertEquals(18.4, z.roomTemp)
    }

    @Test
    fun encodesOnlyProvidedCommandFields() {
        assertEquals("""{"setpoint":21.5}""", FzJson.encodeToString(ZoneCommand.serializer(), ZoneCommand(setpoint = 21.5)))
        assertEquals("""{"mode":"aux_heating"}""", FzJson.encodeToString(ZoneCommand.serializer(), ZoneCommand(mode = Mode.AuxHeating)))
    }

    @Test
    fun normalizesServerUrls() {
        assertEquals("https://breezone.example.fr", normalizeUrl(" breezone.example.fr/ "))
        assertEquals("http://192.168.1.10:8427", normalizeUrl("http://192.168.1.10:8427/"))
    }
}
