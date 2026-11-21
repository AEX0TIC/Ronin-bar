//! Wallpaper packs from GitHub: hand-picked from the community collection
//! [dharmx/walls](https://github.com/dharmx/walls), pinned to one commit so the files can never
//! change under the app. Downloaded on the user's PC for personal use when a pack is chosen;
//! nothing is redistributed. The pictures belong to their artists.
use crate::wallhaven::Wallpaper;

pub const REPO: &str = "dharmx/walls";
/// The commit every file is read from.
pub const COMMIT: &str = "e8d8a42e4fbecb2cf9619995edbc7ce19610bbd8";

/// A named set of pictures (paths inside the repository).
#[derive(Clone, Copy, Debug)]
pub struct Pack {
    pub name: &'static str,
    pub files: &'static [&'static str],
}

pub const PACKS: [Pack; 8] = [
    Pack { name: "Anime art", files: &[
        "anime/a_beach_with_a_bridge_and_trees.jpg",
        "anime/a_building_with_boats_in_the_water.jpg",
        "anime/a_cartoon_of_a_airship_flying_over_a_mountain.png",
        "anime/a_cartoon_of_a_flying_ship_with_buildings_and_a_person_walking.jpg",
        "anime/a_cartoon_of_a_woman_sitting_at_a_table_on_a_deck.jpg",
        "anime/a_cartoon_of_a_woman_standing_on_a_railing_overlooking_a_body_of_water.jpg",
        "anime/a_cat_lying_on_a_piano.jpg",
        "anime/a_forest_with_tall_trees_and_grass.jpeg",
        "anime/a_house_with_a_bike_and_a_chair.jpg",
        "anime/a_log_cabin_in_the_woods.png",
        "anime/a_park_with_a_playground_and_trees_01.jpg",
        "anime/a_person_standing_on_a_broken_object_in_a_field_with_a_large_white_moon.png",
        "anime/a_road_with_power_lines_and_power_lines_01.jpg",
        "anime/a_statue_of_a_man_in_a_park_with_flags_01.jpg",
        "anime/a_train_going_through_a_lake.jpg",
        "anime/a_white_picket_fence_with_flowers_and_a_bridge_over_water.jpg",
    ] },
    Pack { name: "Painted skies", files: &[
        "digital/a_car_driving_on_a_road_at_night.png",
        "digital/a_car_on_a_road_with_orange_clouds_in_the_sky.jpg",
        "digital/a_car_on_the_road.png",
        "digital/a_city_skyline_with_a_plane_in_the_sky.png",
        "digital/a_group_of_people_standing_on_a_mountain.png",
        "digital/a_man_standing_in_a_dark_place_with_a_large_circle_of_light.jpg",
        "digital/a_night_sky_with_clouds_and_a_street_light.png",
        "digital/a_road_with_lightning_bolts_in_the_sky.png",
        "digital/a_sunset_over_a_lake.png",
        "digital/a_truck_in_the_sunset.png",
    ] },
    Pack { name: "Mountains", files: &[
        "mountain/a_castle_on_a_hill_with_fog_with_Eltz_Castle_in_the_background.jpg",
        "mountain/a_iceberg_in_the_water_with_mountains_in_the_background.jpg",
        "mountain/a_large_rock_formation_on_a_beach.jpg",
        "mountain/a_mountain_range_with_dark_clouds.jpg",
        "mountain/a_mountain_range_with_snow_on_top_01.jpg",
        "mountain/a_mountain_with_smoke_coming_out_of_it.jpg",
        "mountain/a_mountain_with_snow_on_top_and_trees_in_the_background.jpg",
        "mountain/a_snowy_mountain_range_with_trees.jpg",
        "mountain/a_snowy_mountain_tops_with_blue_sky.jpg",
        "mountain/a_snowy_mountain_with_a_lake_in_the_middle.jpg",
    ] },
    Pack { name: "Fog & forests", files: &[
        "fogsmoke/a_car_on_a_road_with_fog.jpg",
        "fogsmoke/a_foggy_forest_with_trees_02.jpg",
        "fogsmoke/a_foggy_forest_with_trees_03.jpg",
        "fogsmoke/a_foggy_forest_with_trees_04.jpg",
        "fogsmoke/a_foggy_forest_with_trees_05.jpg",
        "fogsmoke/a_foggy_mountain_with_trees.jpg",
        "fogsmoke/a_forest_of_trees_with_fog.jpg",
        "fogsmoke/a_group_of_trees_in_the_fog_01.jpg",
        "fogsmoke/a_group_of_trees_in_the_fog_02.jpg",
        "fogsmoke/foggy_forest_with_clouds_in_the_sky.jpg",
    ] },
    Pack { name: "Nature", files: &[
        "nature/a_beach_with_waves_and_cliffs.jpg",
        "nature/a_close_up_of_red_leaves.jpg",
        "nature/a_forest_of_trees_with_fog.jpg",
        "nature/a_group_of_green_leaves.jpg",
        "nature/a_lake_with_snow_covered_mountains_in_the_background.jpg",
        "nature/a_mountain_range_with_snow_on_top.jpg",
        "nature/a_road_with_lights_on_the_side_of_a_body_of_water.jpg",
        "nature/a_stairs_leading_to_a_rocky_canyon_with_Flume_Gorge_in_the_background.jpg",
        "nature/a_tall_mountain_with_clouds.jpg",
        "nature/rocks_in_the_water.jpg",
    ] },
    Pack { name: "Minimal", files: &[
        "minimal/a_blue_and_pink_sky_over_a_mountain_range.png",
        "minimal/a_cartoon_of_a_island_with_a_bridge_and_trees_01.jpg",
        "minimal/a_cartoon_of_trees_on_a_hill.png",
        "minimal/a_cartoon_of_trees_on_a_hill_02.png",
        "minimal/a_colorful_snow_covered_land.png",
        "minimal/a_group_of_white_pencils.png",
        "minimal/a_mountain_with_trees_and_moon.jpg",
        "minimal/a_purple_and_orange_mountain_range.png",
        "minimal/a_train_crossing_a_bridge_over_a_bridge_with_mountains_in_the_background.png",
        "minimal/a_video_game_screen_of_a_video_game_01.jpg",
    ] },
    Pack { name: "Flowers", files: &[
        "flowers/a_basket_of_flowers.jpg",
        "flowers/a_close_up_of_a_flower_12.jpg",
        "flowers/a_close_up_of_flowers_01.jpg",
        "flowers/a_close_up_of_purple_flowers.jpg",
        "flowers/a_close_up_of_yellow_flowers.jpg",
        "flowers/a_group_of_flowers_on_a_black_background.jpg",
        "flowers/a_group_of_pink_flowers_on_a_tree.jpg",
        "flowers/a_pink_flower_on_a_pond.jpg",
        "flowers/a_tree_with_white_flowers.jpg",
        "flowers/a_white_flower_with_water_drops_on_it.jpg",
    ] },
    Pack { name: "Cities", files: &[
        "architecture/Osaka_Castle_with_a_green_roof.png",
        "architecture/a_city_at_night_with_lights.jpg",
        "architecture/a_city_in_the_snow.jpg",
        "architecture/a_city_skyline_with_a_sunset_behind_it.png",
        "architecture/a_city_with_tall_buildings.jpg",
        "architecture/a_group_of_birds_flying_in_the_sky_over_a_city.jpg",
        "architecture/a_landscape_with_trees_and_buildings_in_the_distance.jpg",
        "architecture/a_person_walking_on_a_street_between_two_tall_buildings.jpg",
        "architecture/a_street_with_buildings_in_the_background.png",
        "architecture/a_train_tracks_in_a_tunnel.jpg",
    ] },
];

/// A short id for a file (letters and digits, starting with "g"), stable across versions.
pub fn id(file: &str) -> String {
    // FNV-1a, 64 bit, written in base 36.
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in file.bytes() { h ^= u64::from(b); h = h.wrapping_mul(0x0100_0000_01b3); }
    let mut s = String::from("g");
    for _ in 0..10 { s.push(char::from_digit((h % 36) as u32, 36).unwrap_or('0')); h /= 36; }
    s
}

/// The pack's pictures as wallpapers (no preview link: previews are made from the picture).
pub fn wallpapers(p: &Pack) -> Vec<Wallpaper> {
    p.files.iter().map(|f| Wallpaper {
        id: id(f),
        collection: p.name.to_string(),
        url: format!("https://raw.githubusercontent.com/{REPO}/{COMMIT}/{f}"),
        thumb: String::new(),
        page: format!("https://github.com/{REPO}/blob/{COMMIT}/{f}"),
        resolution: String::new(),
    }).collect()
}

/// Every wallpaper of every pack.
pub fn all() -> Vec<Wallpaper> { PACKS.iter().flat_map(wallpapers).collect() }

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn files_are_safe_pinned_pictures() {
        assert_eq!(COMMIT.len(), 40);
        assert!(COMMIT.chars().all(|c| c.is_ascii_hexdigit()));
        for p in PACKS {
            assert!(p.files.len() >= 10, "{}", p.name);
            for f in p.files {
                assert!(f.chars().all(|c| c.is_ascii_alphanumeric() || "_-./".contains(c)), "{f}");
                assert!(!f.contains("..") && !f.starts_with('/'), "{f}");
                let l = f.to_ascii_lowercase();
                assert!(l.ends_with(".jpg") || l.ends_with(".jpeg") || l.ends_with(".png"), "{f}");
            }
        }
    }

    #[test]
    fn ids_are_unique_and_valid() {
        let all = all();
        assert!(all.len() >= 80);
        for (i, w) in all.iter().enumerate() {
            assert!(crate::wallhaven::is_id(&w.id), "{}", w.id);
            assert!(all.iter().skip(i + 1).all(|o| o.id != w.id), "duplicate {}", w.id);
            assert!(w.url.starts_with("https://raw.githubusercontent.com/dharmx/walls/e8d8a42e"));
            assert!(crate::art::original_theme(&w.id).is_none());
        }
        assert_eq!(id("anime/x.jpg"), id("anime/x.jpg"));
    }
}
