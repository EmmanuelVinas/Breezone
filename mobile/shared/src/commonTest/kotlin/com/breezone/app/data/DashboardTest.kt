package com.breezone.app.data

import kotlinx.serialization.json.JsonNull
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.jsonArray
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertTrue

class DashboardTest {
    @Test
    fun aLayoutIsSentWithEveryField() {
        // The server refuses a layout without its version, or a card without its size or config.
        val text = FzJson.encodeToString(DashboardLayout.serializer(), DashboardLayout(blocks = listOf(DashboardBlock("a", "weather"))))
        val json = FzJson.parseToJsonElement(text).jsonObject
        assertEquals(1, json["version"]!!.jsonPrimitive.content.toInt())
        val card = json["blocks"]!!.jsonArray.single().jsonObject
        assertEquals("full", card["size"]!!.jsonPrimitive.content)
        assertTrue("config" in card)
    }

    @Test
    fun aCardOfTheWebKeepsItsShareOfARowOnAPhone() {
        assertEquals(BlockSize.Half, BlockSize.of("quarter"))
        assertEquals(BlockSize.Half, BlockSize.of("half"))
        assertEquals(BlockSize.Full, BlockSize.of("full"))
        val b = DashboardBlock("x", "group", config = kotlinx.serialization.json.JsonObject(mapOf("room_id" to JsonNull, "hidden" to kotlinx.serialization.json.JsonArray(listOf(JsonPrimitive(4))))))
        assertEquals(null, b.long("room_id"))
        assertEquals(listOf(4L), b.longs("hidden"))
    }

    @Test
    fun theSuggestedHomeShowsZonesAloneAsCardsAndRoomsOtherwise() {
        val zone = DeviceView(1, kind = "climate.zone", traits = listOf("climate.zone@1"), source = "airzone", installationId = 1)
        val alone = generateMobileLayout(HomeFacts(listOf(zone), emptyList(), null, hasZones = true, hasRoutines = false)).blocks
        assertEquals(listOf(BlockTypes.PAGES, BlockTypes.WEATHER, BlockTypes.ZONES), alone.map { it.type })

        val light = DeviceView(2, kind = "light", traits = listOf(TraitIds.LIGHT), source = "hue", sourceId = 3, roomId = 7)
        val rooms = listOf(Room(7, "Salon"))
        val home = generateMobileLayout(HomeFacts(listOf(zone, light), rooms, EnergyRoles(solar = true), hasZones = true, hasRoutines = true)).blocks
        assertEquals(
            listOf(BlockTypes.PAGES, BlockTypes.ENERGY, BlockTypes.ENERGY, BlockTypes.WEATHER, BlockTypes.GROUP, BlockTypes.GROUP, BlockTypes.SCENES),
            home.map { it.type },
        )
        assertEquals(7L, home[4].long("room_id"))
        // The same ids from one composition to the next: the list keys its cards on them.
        assertEquals(home.map { it.id }, generateMobileLayout(HomeFacts(listOf(zone, light), rooms, EnergyRoles(solar = true), hasZones = true, hasRoutines = true)).blocks.map { it.id })
    }
}
