package com.breezone.app.data

import kotlinx.serialization.Serializable
import kotlinx.serialization.json.JsonObject

/** A value of a trait, or a parameter of one of its actions (`GET /api/traits`). */
@Serializable
data class CatalogField(
    val name: String,
    /** "bool" | "integer" | "number" | "string" | "enum" */
    val type: String = "string",
    val unit: String? = null,
    val values: List<String> = emptyList(),
    val valueLabels: Map<String, ModuleLabel> = emptyMap(),
    val min: Double? = null,
    val max: Double? = null,
    val optional: Boolean = false,
    val label: ModuleLabel = ModuleLabel(),
)

@Serializable
data class CatalogAction(val name: String, val label: ModuleLabel = ModuleLabel(), val params: List<CatalogField> = emptyList())

/** A piece of a card of a trait (see `server/contracts/ui.json`). */
@Serializable
data class CatalogWidget(
    val widget: String,
    val field: String? = null,
    val action: String? = null,
    val param: String? = null,
    val params: JsonObject? = null,
    val `when`: String? = null,
)

@Serializable
data class CatalogUi(val kind: String = "other", val primary: String? = null, val card: List<CatalogWidget> = emptyList())

/** A trait of the catalog the server serves: what a kind of device holds, accepts, and how it looks. */
@Serializable
data class CatalogTrait(
    val trait: String,
    val version: Int,
    val state: List<CatalogField> = emptyList(),
    val actions: List<CatalogAction> = emptyList(),
    val ui: CatalogUi? = null,
)

/** The trait of the catalog a device has (`name@version`). */
fun List<CatalogTrait>.traitOf(id: String): CatalogTrait? {
    val (name, version) = id.split('@').let { it[0] to it.getOrNull(1) }
    return firstOrNull { it.trait == name && it.version.toString() == version }
}
