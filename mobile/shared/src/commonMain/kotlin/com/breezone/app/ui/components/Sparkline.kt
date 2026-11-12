package com.breezone.app.ui.components

import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.remember
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.unit.dp
import com.patrykandpatrick.vico.multiplatform.cartesian.CartesianChartHost
import com.patrykandpatrick.vico.multiplatform.cartesian.Zoom
import com.patrykandpatrick.vico.multiplatform.cartesian.data.CartesianChartModelProducer
import com.patrykandpatrick.vico.multiplatform.cartesian.data.CartesianLayerRangeProvider
import com.patrykandpatrick.vico.multiplatform.cartesian.data.lineSeries
import com.patrykandpatrick.vico.multiplatform.cartesian.layer.LineCartesianLayer
import com.patrykandpatrick.vico.multiplatform.cartesian.layer.rememberLine
import com.patrykandpatrick.vico.multiplatform.cartesian.layer.rememberLineCartesianLayer
import com.patrykandpatrick.vico.multiplatform.cartesian.rememberCartesianChart
import com.patrykandpatrick.vico.multiplatform.cartesian.rememberVicoScrollState
import com.patrykandpatrick.vico.multiplatform.cartesian.rememberVicoZoomState
import com.patrykandpatrick.vico.multiplatform.common.Fill
import kotlin.math.round

/** A small line of `[ts, value]` points (oldest first), with a faint area below it. */
@Composable
fun Sparkline(points: List<List<Double>>, color: Color, modifier: Modifier = Modifier) {
    val valid = remember(points) { points.filter { it.size >= 2 } }
    val producer = remember { CartesianChartModelProducer() }
    LaunchedEffect(valid) {
        if (valid.size < 2) return@LaunchedEffect
        val t0 = valid.first()[0]
        producer.runTransaction {
            lineSeries { series(valid.map { round((it[0] - t0) / 36.0) / 100 }, valid.map { it[1] }) }
        }
    }
    if (valid.size < 2) return
    val values = valid.map { it[1] }
    // A flat line sits in the middle rather than at the bottom.
    val pad = (0.5 - (values.max() - values.min())).coerceAtLeast(0.0) / 2
    val line = LineCartesianLayer.rememberLine(
        fill = LineCartesianLayer.LineFill.single(Fill(color)),
        areaFill = LineCartesianLayer.AreaFill.single(Fill(Brush.verticalGradient(listOf(color.copy(alpha = 0.18f), Color.Transparent)))),
        stroke = LineCartesianLayer.LineStroke.Continuous(thickness = 1.75.dp),
        pointConnector = LineCartesianLayer.PointConnector.cubic(),
    )
    CartesianChartHost(
        chart = rememberCartesianChart(
            rememberLineCartesianLayer(
                lineProvider = remember(line) { LineCartesianLayer.LineProvider.series(line) },
                pointSpacing = 2.dp,
                rangeProvider = remember(pad, values) { CartesianLayerRangeProvider.fixed(minY = values.min() - pad, maxY = values.max() + pad) },
            ),
            getXStep = { _, _, _ -> 1.0 },
        ),
        modelProducer = producer,
        modifier = modifier,
        scrollState = rememberVicoScrollState(scrollEnabled = false),
        zoomState = rememberVicoZoomState(zoomEnabled = false, initialZoom = Zoom.Content),
        animateIn = false,
    )
}
