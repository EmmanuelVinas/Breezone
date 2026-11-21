package com.breezone.app.ui.components

import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.remember
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.unit.dp
import com.patrykandpatrick.vico.multiplatform.cartesian.CartesianChartHost
import com.patrykandpatrick.vico.multiplatform.cartesian.Zoom
import com.patrykandpatrick.vico.multiplatform.cartesian.axis.HorizontalAxis
import com.patrykandpatrick.vico.multiplatform.cartesian.axis.VerticalAxis
import com.patrykandpatrick.vico.multiplatform.cartesian.data.CartesianChartModelProducer
import com.patrykandpatrick.vico.multiplatform.cartesian.data.CartesianLayerRangeProvider
import com.patrykandpatrick.vico.multiplatform.cartesian.data.CartesianValueFormatter
import com.patrykandpatrick.vico.multiplatform.cartesian.data.columnSeries
import com.patrykandpatrick.vico.multiplatform.cartesian.data.lineSeries
import com.patrykandpatrick.vico.multiplatform.cartesian.layer.ColumnCartesianLayer
import com.patrykandpatrick.vico.multiplatform.cartesian.layer.rememberColumnCartesianLayer
import com.patrykandpatrick.vico.multiplatform.cartesian.layer.LineCartesianLayer
import com.patrykandpatrick.vico.multiplatform.cartesian.layer.rememberLine
import com.patrykandpatrick.vico.multiplatform.cartesian.layer.rememberLineCartesianLayer
import com.patrykandpatrick.vico.multiplatform.cartesian.rememberCartesianChart
import com.patrykandpatrick.vico.multiplatform.cartesian.rememberVicoScrollState
import com.patrykandpatrick.vico.multiplatform.cartesian.rememberVicoZoomState
import com.patrykandpatrick.vico.multiplatform.common.Fill
import com.patrykandpatrick.vico.multiplatform.common.component.rememberLineComponent
import com.breezone.app.ui.screens.formatPower
import kotlin.math.round
import kotlin.math.roundToInt

/** A series of a power chart: `[ts, W]` points, drawn as an area or a line. */
data class PowerSeries(val points: List<Pair<Long, Double>>, val color: Color, val area: Boolean, val dashed: Boolean = false)

/**
 * The power of a day (kW on the left axis, hours below): each series from [dayStart] (a timestamp),
 * over 24 h. Series without points are left out.
 */
@Composable
fun PowerDayChart(series: List<PowerSeries>, dayStart: Long, modifier: Modifier = Modifier) {
    val shown = remember(series) { series.filter { it.points.size >= 2 } }
    val producer = remember { CartesianChartModelProducer() }
    LaunchedEffect(shown, dayStart) {
        if (shown.isEmpty()) return@LaunchedEffect
        producer.runTransaction {
            lineSeries {
                shown.forEach { s ->
                    series(s.points.map { round((it.first - dayStart) / 36.0) / 100 }, s.points.map { it.second / 1000 })
                }
            }
        }
    }
    if (shown.isEmpty()) return
    val lines = shown.map { s ->
        LineCartesianLayer.rememberLine(
            fill = LineCartesianLayer.LineFill.single(Fill(s.color)),
            areaFill = if (s.area) LineCartesianLayer.AreaFill.single(Fill(Brush.verticalGradient(listOf(s.color.copy(alpha = 0.3f), Color.Transparent)))) else null,
            stroke = if (s.dashed) LineCartesianLayer.LineStroke.Dashed(thickness = 1.5.dp, dashLength = 3.dp, gapLength = 3.dp)
            else LineCartesianLayer.LineStroke.Continuous(thickness = if (s.area) 1.5.dp else 1.75.dp),
            pointConnector = LineCartesianLayer.PointConnector.cubic(),
        )
    }
    val max = shown.flatMap { s -> s.points.map { it.second / 1000 } }.maxOrNull()?.coerceAtLeast(1.0) ?: 1.0
    val hours = remember { CartesianValueFormatter { _, x, _ -> "${x.toInt()} h" } }
    CartesianChartHost(
        chart = rememberCartesianChart(
            rememberLineCartesianLayer(
                lineProvider = remember(lines) { LineCartesianLayer.LineProvider.series(lines) },
                pointSpacing = 3.dp,
                rangeProvider = remember(max) { CartesianLayerRangeProvider.fixed(minX = 0.0, maxX = 24.0, minY = 0.0, maxY = max) },
            ),
            startAxis = VerticalAxis.rememberStart(
                label = rememberAxisLabel(),
                line = null,
                tick = null,
                valueFormatter = remember { CartesianValueFormatter { _, y, _ -> formatPower(y * 1000) } },
                itemPlacer = remember { VerticalAxis.ItemPlacer.count({ 4 }) },
            ),
            bottomAxis = HorizontalAxis.rememberBottom(label = rememberAxisLabel(), guideline = null, valueFormatter = hours, itemPlacer = remember { HorizontalAxis.ItemPlacer.aligned(spacing = { 6 }) }),
            getXStep = { _, _, _ -> 1.0 },
        ),
        modelProducer = producer,
        modifier = modifier,
        scrollState = rememberVicoScrollState(scrollEnabled = false),
        zoomState = rememberVicoZoomState(zoomEnabled = false, initialZoom = Zoom.Content),
        animateIn = false,
    )
}

/** A curve of a [LinesChart]: `[ts, value]` points and its colour. */
data class TimeSeries(val points: List<List<Double>>, val color: Color)

/**
 * Several curves over the same hours (one per room…), from [from] (a timestamp) over [hours];
 * [label] writes the hours below. Curves without points are left out.
 */
@Composable
fun LinesChart(series: List<TimeSeries>, from: Long, hours: Int, label: (Double) -> String, modifier: Modifier = Modifier) {
    val shown = remember(series) { series.filter { it.points.size >= 2 } }
    val producer = remember { CartesianChartModelProducer() }
    LaunchedEffect(shown, from) {
        if (shown.isEmpty()) return@LaunchedEffect
        producer.runTransaction {
            lineSeries {
                shown.forEach { s -> series(s.points.map { round((it[0] - from) / 36.0) / 100 }, s.points.map { it[1] }) }
            }
        }
    }
    if (shown.isEmpty()) return
    val lines = shown.map { s ->
        LineCartesianLayer.rememberLine(
            fill = LineCartesianLayer.LineFill.single(Fill(s.color)),
            stroke = LineCartesianLayer.LineStroke.Continuous(thickness = 1.75.dp),
            pointConnector = LineCartesianLayer.PointConnector.cubic(),
        )
    }
    val values = shown.flatMap { s -> s.points.map { it[1] } }
    val min = values.minOrNull() ?: 0.0
    val max = (values.maxOrNull() ?: 1.0).coerceAtLeast(min + 1)
    val formatter = remember(label) { CartesianValueFormatter { _, x, _ -> label(x) } }
    CartesianChartHost(
        chart = rememberCartesianChart(
            rememberLineCartesianLayer(
                lineProvider = remember(lines) { LineCartesianLayer.LineProvider.series(lines) },
                pointSpacing = 3.dp,
                rangeProvider = remember(min, max, hours) { CartesianLayerRangeProvider.fixed(minX = 0.0, maxX = hours.toDouble(), minY = min.coerceAtMost(max - 1), maxY = max) },
            ),
            startAxis = VerticalAxis.rememberStart(label = rememberAxisLabel()),
            bottomAxis = HorizontalAxis.rememberBottom(
                label = rememberAxisLabel(),
                valueFormatter = formatter,
                itemPlacer = remember(hours) { HorizontalAxis.ItemPlacer.aligned(spacing = { if (hours > 24) 24 else 6 }) },
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

/** A bar of an [EnergyBarsChart]: its label below, the energy used and made (kWh). */
data class EnergyBar(val label: String, val consumption: Double, val production: Double?)

/** Day by day (or month by month): the energy used, and beside it the energy made when measured. */
@Composable
fun EnergyBarsChart(bars: List<EnergyBar>, consumed: Color, produced: Color, modifier: Modifier = Modifier) {
    val solar = bars.any { (it.production ?: 0.0) > 0 }
    val producer = remember { CartesianChartModelProducer() }
    LaunchedEffect(bars, solar) {
        if (bars.isEmpty()) return@LaunchedEffect
        producer.runTransaction {
            columnSeries {
                series(bars.indices.toList(), bars.map { it.consumption })
                if (solar) series(bars.indices.toList(), bars.map { it.production ?: 0.0 })
            }
        }
    }
    if (bars.isEmpty()) return
    val thickness = if (bars.size > 14) 4.dp else 8.dp
    val shape = RoundedCornerShape(topStart = 3.dp, topEnd = 3.dp)
    val columns = listOfNotNull(
        rememberLineComponent(Fill(consumed), thickness = thickness, shape = shape),
        if (solar) rememberLineComponent(Fill(produced), thickness = thickness, shape = shape) else null,
    )
    val marker = rememberTextMarker(bars) { x ->
        val b = bars.getOrNull(x.roundToInt()) ?: return@rememberTextMarker ""
        buildString {
            append(b.label)
            append("\n${kwh(b.consumption)}")
            b.production?.takeIf { solar }?.let { append(" · ☀ ${kwh(it)}") }
        }
    }
    CartesianChartHost(
        chart = rememberCartesianChart(
            rememberColumnCartesianLayer(
                columnProvider = remember(columns) { ColumnCartesianLayer.ColumnProvider.series(columns) },
                columnCollectionSpacing = if (bars.size > 14) 2.dp else 6.dp,
                rangeProvider = remember { CartesianLayerRangeProvider.fixed(minY = 0.0) },
            ),
            startAxis = VerticalAxis.rememberStart(
                label = rememberAxisLabel(),
                line = null,
                tick = null,
                valueFormatter = remember { CartesianValueFormatter { _, y, _ -> "${y.roundToInt()}" } },
            ),
            bottomAxis = HorizontalAxis.rememberBottom(
                label = rememberAxisLabel(),
                guideline = null,
                valueFormatter = remember(bars) { CartesianValueFormatter { _, x, _ -> bars.getOrNull(x.roundToInt())?.label.orEmpty() } },
                itemPlacer = remember(bars.size) { HorizontalAxis.ItemPlacer.aligned(spacing = { if (bars.size > 14) (bars.size / 6).coerceAtLeast(1) else 1 }) },
            ),
            marker = marker,
        ),
        modelProducer = producer,
        modifier = modifier,
        scrollState = rememberVicoScrollState(scrollEnabled = false),
        zoomState = rememberVicoZoomState(zoomEnabled = false, initialZoom = Zoom.Content),
        animateIn = false,
    )
}

private fun kwh(v: Double): String = "${(v * 10).roundToInt() / 10.0} kWh"
