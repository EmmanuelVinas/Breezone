package com.breezone.app.ui.components

import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.MaterialTheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.remember
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.breezone.app.ui.theme.Fz
import com.patrykandpatrick.vico.multiplatform.cartesian.axis.rememberAxisGuidelineComponent
import com.patrykandpatrick.vico.multiplatform.cartesian.marker.DefaultCartesianMarker
import com.patrykandpatrick.vico.multiplatform.cartesian.marker.rememberDefaultCartesianMarker
import com.patrykandpatrick.vico.multiplatform.common.Fill
import com.patrykandpatrick.vico.multiplatform.common.Insets
import com.patrykandpatrick.vico.multiplatform.common.component.TextComponent
import com.patrykandpatrick.vico.multiplatform.common.component.rememberShapeComponent
import com.patrykandpatrick.vico.multiplatform.common.component.rememberTextComponent

/** Small muted labels of the chart axes. */
@Composable
fun rememberAxisLabel(): TextComponent = rememberTextComponent(style = TextStyle(fontSize = 10.sp, color = Fz.colors.muted))

/**
 * Marker shown while the chart is touched: a card with the text [format] gives for the touched
 * x-value (several lines allowed).
 */
@Composable
fun rememberTextMarker(vararg keys: Any?, format: (x: Double) -> String): DefaultCartesianMarker {
    val label = rememberTextComponent(
        style = TextStyle(fontSize = 12.sp, color = MaterialTheme.colorScheme.onSurface),
        lineCount = 6,
        padding = Insets(8.dp, 6.dp),
        background = rememberShapeComponent(
            fill = Fill(MaterialTheme.colorScheme.surfaceContainerHigh),
            shape = RoundedCornerShape(10.dp),
        ),
    )
    return rememberDefaultCartesianMarker(
        label = label,
        // Next to the touched point: no room is kept for it above the chart.
        labelPosition = DefaultCartesianMarker.LabelPosition.AroundPoint,
        guideline = rememberAxisGuidelineComponent(),
        valueFormatter = remember(*keys) {
            DefaultCartesianMarker.ValueFormatter { _, targets -> targets.firstOrNull()?.let { format(it.x) }.orEmpty() }
        },
    )
}
