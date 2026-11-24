import SwiftUI
import UIKit
import WatchCore

/// A target drawn on the map.
struct MapMarker: Equatable {
    var id: String
    var position: LatLon
    var selected: Bool
    var stale: Bool
    /// The station's predicted position (no signal): a dashed pink ring.
    var predicted = false
}

/// What the map shows: centre, zoom, rotation (degrees clockwise; the
/// negated heading for heading-up) and the watch position.
struct MapViewState: Equatable {
    var center: LatLon
    var zoom: Double
    var rotationDeg: Double
    var user: LatLon?
}

/// Station map tiles under the compass (docs/watch-companion.md "Map
/// background"): the cached tiles for the view (an ancestor tile cut to
/// size where a level is missing), the targets, the bearing line from the
/// watch to the selected target, a north tick and the imagery credit. Draws
/// nothing when no tile covers the view, so the plain compass shows.
/// `onVisible` gets the keys of the view's tiles (to fetch missing ones in
/// direct mode).
struct MapBackground: View {
    @ObservedObject var tiles: TileStore
    let state: MapViewState
    let markers: [MapMarker]
    let onVisible: ([String]) -> Void

    private struct Source {
        var left: Double
        var top: Double
        var image: UIImage
        var crop: AncestorCrop?
    }

    var body: some View {
        GeometryReader { geo in
            let visible = MapTiles.visibleTiles(lat: state.center.lat, lon: state.center.lon, zoom: state.zoom,
                                                width: Double(geo.size.width), height: Double(geo.size.height))
            let keys = visible.tiles.map(\.tile.key)
            let sources = visible.tiles.compactMap { p -> Source? in
                if tiles.keys.contains(p.tile.key), let image = tiles.image(p.tile.key) {
                    return Source(left: p.left, top: p.top, image: image, crop: nil)
                }
                guard let crop = MapTiles.ancestor(p.tile, available: { tiles.keys.contains($0) }),
                      let image = tiles.image(crop.key)
                else { return nil }
                return Source(left: p.left, top: p.top, image: image, crop: crop)
            }
            ZStack(alignment: .bottom) {
                if !sources.isEmpty {
                    Canvas { context, size in
                        draw(&context, size: size, sources: sources, tilePx: visible.tilePx)
                    }
                    // The tile licence asks for the imagery credit.
                    Text(tiles.attribution)
                        .font(.system(size: 8))
                        .foregroundStyle(.white.opacity(0.7))
                        .lineLimit(1)
                        .minimumScaleFactor(0.6)
                        .padding(.bottom, 1)
                }
            }
            .frame(width: geo.size.width, height: geo.size.height)
            .onAppear { onVisible(keys) }
            .onChange(of: keys) { onVisible(keys) }
        }
        .allowsHitTesting(false)
    }

    private func color(_ id: String) -> Color {
        switch id {
        case "rocket": return Color(red: 1.0, green: 0.44, blue: 0.26)
        case "ground_station": return Color(red: 0.4, green: 0.73, blue: 0.42)
        case "rrls": return Color(red: 0.26, green: 0.65, blue: 0.96)
        default: return .white
        }
    }

    private func draw(_ context: inout GraphicsContext, size: CGSize, sources: [Source], tilePx: Double) {
        let c = CGPoint(x: size.width / 2, y: size.height / 2)
        var map = context
        map.translateBy(x: c.x, y: c.y)
        map.rotate(by: .degrees(state.rotationDeg))
        let edge = tilePx + 1
        for s in sources {
            let rect = CGRect(x: s.left, y: s.top, width: edge, height: edge)
            if let crop = s.crop {
                // The ancestor scaled up so its sub-square fills this tile.
                let full = tilePx / crop.size
                let ancestorRect = CGRect(x: s.left - crop.x * full, y: s.top - crop.y * full, width: full, height: full)
                map.drawLayer { layer in
                    layer.clip(to: Path(rect))
                    layer.draw(Image(uiImage: s.image), in: ancestorRect)
                }
            } else {
                map.draw(Image(uiImage: s.image), in: rect)
            }
        }
        // Dim the imagery so the compass text stays readable.
        context.fill(Path(CGRect(origin: .zero, size: size)), with: .color(.black.opacity(0.35)))

        let r = state.rotationDeg * Double.pi / 180
        func screen(_ p: LatLon) -> CGPoint {
            let d = MapTiles.project(lat: p.lat, lon: p.lon, centerLat: state.center.lat, centerLon: state.center.lon,
                                     zoom: state.zoom)
            return CGPoint(x: c.x + d.dx * cos(r) - d.dy * sin(r), y: c.y + d.dx * sin(r) + d.dy * cos(r))
        }
        let user = state.user.map(screen)
        if let user, let selected = markers.first(where: \.selected) {
            var line = Path()
            line.move(to: user)
            line.addLine(to: screen(selected.position))
            if selected.predicted {
                context.stroke(line, with: .color(Color.predicted.opacity(0.9)),
                               style: StrokeStyle(lineWidth: 2, dash: [5, 4]))
            } else {
                context.stroke(line, with: .color(.white.opacity(0.8)), lineWidth: 2)
            }
        }
        for m in markers {
            let at = screen(m.position)
            if m.predicted {
                // Hollow dashed ring: an estimate, not a fix.
                let radius: CGFloat = m.selected ? 8 : 6
                let ring = Path(ellipseIn: CGRect(x: at.x - radius, y: at.y - radius, width: radius * 2, height: radius * 2))
                context.stroke(ring, with: .color(.black.opacity(0.6)), lineWidth: 4)
                context.stroke(ring, with: .color(Color.predicted), style: StrokeStyle(lineWidth: 2.5, dash: [4, 3]))
                continue
            }
            let radius: CGFloat = m.selected ? 6 : 4.5
            let dot = Path(ellipseIn: CGRect(x: at.x - radius, y: at.y - radius, width: radius * 2, height: radius * 2))
            context.fill(dot, with: .color(color(m.id).opacity(m.stale ? 0.5 : 1)))
            context.stroke(dot, with: .color(.black), lineWidth: 1.5)
        }
        if let user {
            context.fill(Path(ellipseIn: CGRect(x: user.x - 4.5, y: user.y - 4.5, width: 9, height: 9)), with: .color(.white))
            context.fill(Path(ellipseIn: CGRect(x: user.x - 3, y: user.y - 3, width: 6, height: 6)), with: .color(.cyan))
        }
        // North tick on the rim.
        var north = context
        north.translateBy(x: c.x, y: c.y)
        north.rotate(by: .degrees(state.rotationDeg))
        let rim = min(size.width, size.height) / 2
        var tick = Path()
        tick.move(to: CGPoint(x: 0, y: -rim + 2))
        tick.addLine(to: CGPoint(x: 0, y: -rim + 12))
        north.stroke(tick, with: .color(.red), lineWidth: 3)
    }
}
