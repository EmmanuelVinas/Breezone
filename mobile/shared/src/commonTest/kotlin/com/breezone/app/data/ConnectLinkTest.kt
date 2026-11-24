package com.breezone.app.data

import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertNull

class ConnectLinkTest {
    @Test
    fun readsTheServerAndTheUser() {
        assertEquals(
            ConnectLink("https://breezone.example.org", "camille"),
            ConnectLinks.parse("breezone://connect?url=https%3A%2F%2Fbreezone.example.org%2F&username=camille"),
        )
        assertEquals(ConnectLink("http://192.168.1.20:8427", ""), ConnectLinks.parse("breezone://connect?url=http%3A%2F%2F192.168.1.20%3A8427"))
    }

    @Test
    fun refusesOtherLinks() {
        assertNull(ConnectLinks.parse("https://connect?url=https%3A%2F%2Fa.org"))
        assertNull(ConnectLinks.parse("breezone://other?url=https%3A%2F%2Fa.org"))
        assertNull(ConnectLinks.parse("breezone://connect?username=camille"))
        assertNull(ConnectLinks.parse("breezone://connect?url=javascript%3Aalert(1)"))
        assertNull(ConnectLinks.parse("breezone://connect?url=a.org"))
    }
}
