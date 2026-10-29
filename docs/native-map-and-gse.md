# Native map and GSE diagnostics

ARM Linux native builds default to the existing 2D raster map renderer because
some Raspberry Pi WebKit installations show a blank WebGL canvas despite
successful tile delivery. Browser builds keep MapLibre. The raster renderer
preserves tiles and position markers but does not offer all WebGL interactions.
Set `GS26_MAP_RENDERER=webgl` to retry WebGL, or `raster` to explicitly select
the compatibility renderer. Native tile fetching has bounded connection and
request timeouts. Validate the rendered map on the target Pi after deployment;
the mocked DOM tests do not exercise its GPU/WebKit stack.

Tabs retain horizontal scrolling but hide the overlapping scrollbar. Network
nodes show last-seen time and elapsed age, including offline known connections.

The GSE action panel shows backend rejection/configuration diagnostics. Configure
the maximum acceptable empty-tank PT offset in GSE settings before automation.
Pressure failure thresholds are nitrogen target +100 psi and nitrous target
+50 psi, constrained by any lower optional hardware ceiling. All interlocks
and sensor freshness prerequisites remain enforced.
