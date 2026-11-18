//! The world as GeoJSON: one polygon per place with its owner, maps, presence and units, styled
//! with simplestyle properties (`fill`, `stroke`) so it shows coloured in most viewers.

use serde_json::{json, Map, Value};

use crate::world::World;

fn round(v: f64, k: f64) -> f64 {
    (v * k).round() / k
}

pub fn to_geojson(w: &World) -> Value {
    let g = w.game;
    let mut features = Vec::with_capacity(w.city.len());
    for (p, place) in w.city.places.iter().enumerate() {
        let mut ring: Vec<Value> = place
            .ring
            .iter()
            .map(|&pt| {
                let ll = w.city.lonlat(pt);
                json!([round(ll[0], 1e7), round(ll[1], 1e7)])
            })
            .collect();
        if let Some(first) = ring.first().cloned() {
            ring.push(first);
        }
        let owner = w.owner[p];
        let mut props = Map::new();
        props.insert("id".into(), json!(place.id));
        props.insert("name".into(), json!(place.name));
        props.insert(
            "owner".into(),
            owner.map_or(Value::Null, |f| json!(g.factions[f].id)),
        );
        props.insert(
            "owner_name".into(),
            owner.map_or(Value::Null, |f| json!(g.factions[f].name)),
        );
        props.insert(
            "fill".into(),
            json!(owner.map_or("#d8d8d8", |f| g.factions[f].color.as_str())),
        );
        props.insert(
            "fill-opacity".into(),
            json!(if owner.is_some() { 0.65 } else { 0.25 }),
        );
        props.insert("stroke".into(), json!("#555555"));
        props.insert("stroke-width".into(), json!(0.5));
        for (m, def) in g.rules.maps.iter().enumerate() {
            props.insert(def.id.clone(), json!(round(w.gb.maps[m][p], 100.0)));
        }
        let mut presence = Map::new();
        for &h in &w.gangs {
            let v = w.presence[h][p];
            if v > 0.5 {
                presence.insert(g.factions[h].id.clone(), json!(round(v, 10.0)));
            }
        }
        props.insert("presence".into(), Value::Object(presence));
        let units: Vec<Value> = w.units_at[p]
            .iter()
            .map(|&u| {
                let unit = &w.gb.units[u];
                let who = g
                    .factions
                    .get(unit.account.wrapping_sub(1))
                    .map_or("public", |f| f.id.as_str());
                json!(format!("{} ({who})", g.rules.units[unit.def].id))
            })
            .collect();
        props.insert("units".into(), Value::Array(units));
        features.push(json!({ "type": "Feature", "properties": props, "geometry": { "type": "Polygon", "coordinates": [ring] } }));
    }
    let factions: Vec<Value> = w
        .factions
        .iter()
        .enumerate()
        .map(|(f, s)| json!({ "id": g.factions[f].id, "name": g.factions[f].name, "color": g.factions[f].color, "territory": s.territory, "alive": s.alive }))
        .collect();
    json!({
        "type": "FeatureCollection",
        "name": format!("{} (seed {}, day {})", g.name, w.seed, w.day),
        "lilienstern": { "game": g.name, "seed": w.seed, "day": w.day, "factions": factions },
        "features": features,
    })
}
