package com.breezone.app.data

import kotlinx.serialization.Serializable

/** A day as Enedis measured it (Linky), its cost, and what the modules measured of it. */
@Serializable
data class LinkyDay(
    val day: String,
    val consumption: Double? = null,
    val production: Double? = null,
    val cost: Double? = null,
    val measured: Double? = null,
)

/** What a linked Linky account gives (`GET /api/linky`): no PRM without an account. */
@Serializable
data class LinkyView(val prm: String? = null, val days: List<LinkyDay> = emptyList())
