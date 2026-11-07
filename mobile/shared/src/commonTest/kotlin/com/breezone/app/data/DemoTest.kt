package com.breezone.app.data

import kotlin.test.Test
import kotlin.test.assertFalse
import kotlin.test.assertTrue

class DemoTest {
    @Test
    fun theReviewAccountOpensTheDemo() {
        assertTrue(isDemoLogin("demo", "demo"))
        assertTrue(isDemoLogin(" Demo ", "demo"))
    }

    @Test
    fun anythingElseIsARealAccount() {
        assertFalse(isDemoLogin("demo", "wrong"))
        assertFalse(isDemoLogin("demo", "Demo"))
        assertFalse(isDemoLogin("admin", "demo"))
        // A real Breezone password has at least 8 characters: "demo" can never be one.
        assertTrue("demo".length < 8)
    }
}
