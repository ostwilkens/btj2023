#![allow(clippy::too_many_arguments, clippy::type_complexity)]

pub mod ai;
mod bullets;
pub mod despawn_after;
pub mod draw;
pub mod movement;
pub mod player;
pub mod utils;

use std::f32::consts::{PI, TAU};

use bevy::{
    core_pipeline::{bloom::BloomSettings, tonemapping::Tonemapping},
    math::{Vec3Swizzles, vec2},
    prelude::*, render::camera::ScalingMode,
};
use bevy_vector_shapes::prelude::*;

use ai::*;
use bullets::*;
use despawn_after::*;
use draw::*;
use movement::*;
use player::*;

fn main() {
    App::new()
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                fit_canvas_to_parent: true,
                ..default()
            }),
            ..default()
        }))
        .add_plugins(DespawnAfterPlugin)
        .add_plugins(Game)
        .run();
}
#[derive(Component, Debug)]
pub struct RemoveOnRespawn;

#[derive(Component, Debug)]
pub struct Health {
    pub current: f32,
    pub max: f32,
}
#[derive(Component, Debug)]
pub struct Cooldown {
    pub start_time: f32,
    pub duration: f32,
}
impl Cooldown {
    fn is_ready(&self, elapsed_seconds: f32) -> bool {
        self.start_time + self.duration < elapsed_seconds
    }
}

pub struct Game;

#[derive(Component, Clone)]
pub struct TeamIdx(pub usize);

#[derive(Resource)]
pub struct Teams {
    pub colors: Vec<(Color, Color)>,
}

impl Default for Teams {
    fn default() -> Self {
        Self {
            colors: vec![
                (Color::WHITE * 5f32, Color::GREEN * 5f32),
                (Color::ORANGE * 5f32, Color::RED * 5f32),
            ],
        }
    }
}

#[derive(Component, Clone)]
pub struct HealthPickup(pub f32);

#[derive(Event)]
pub struct EventTryApplyDamages(pub Entity, pub f32);

#[derive(Resource)]
pub struct GameDef {
    pub spawn_interval: f32,
    pub spawn_interval_multiplier_per_second: f32,
}

impl Default for GameDef {
    fn default() -> Self {
        Self {
            spawn_interval: 5f32,
            spawn_interval_multiplier_per_second: 0.9f32,
        }
    }
}

impl Plugin for Game {
    fn build(&self, app: &mut App) {
        app.add_plugins(Shape2dPlugin::default());
        app.add_plugins(BulletPlugin);
        app.init_resource::<GameDef>();
        app.init_resource::<Teams>();
        app.add_event::<EventBulletSpawn>();
        app.add_event::<EventTryApplyDamages>();
        app.add_systems(Startup, setup);
        app.add_systems(
            Update,
            (
                (player_respawn),
                (/*handle_mouse_to_move, */ handle_clicks_to_fire, wasd_movement),
                (
                    move_targets,
                    move_direction,
                    spawn_ais,
                    ai::ai_fire,
                    ai::ai_move,
                ),
                (try_apply_damages,),
                (
                    collisions_player_pickups,
                    collisions_bullet_health,
                    draw,
                    draw_bullets,
                    draw_health,
                    draw_cooldown,
                    draw_pickups,
                ),
            )
                .chain(),
        );
    }
}

fn player_respawn(
    mut commands: Commands,
    mut q: ParamSet<(
        Query<Entity, With<Player>>,
        Query<Entity, With<RemoveOnRespawn>>,
    )>,
) {
    if q.p0().iter().next().is_some() {
        return;
    }
    // Remove extra stuff
    for e in q.p1().iter() {
        commands.entity(e).despawn();
    }
    // Spawn player
    commands.spawn((
        Transform {
            translation: Vec2::ZERO.extend(2f32),
            ..default()
        },
        MoveSpeed(130f32),
        MoveDirection(Vec2::ZERO),
        MoveTarget {
            target: Some(Vec2::new(0f32, 0f32)),
        },
        Health {
            current: 1f32,
            max: 1f32,
        },
        Cooldown {
            start_time: 0.0,
            duration: 0.5,
        },
        Player,
        TeamIdx(0),
    ));
}

pub fn setup(mut commands: Commands, asset_server: Res<AssetServer>) {
    commands.spawn((
        Camera2dBundle {
            projection: OrthographicProjection {
                scaling_mode: ScalingMode::AutoMin {
                    min_width: 512.0,
                    min_height: 512.0,
                },
                ..default()
            },
            camera: Camera {
                hdr: true, // 1. HDR is required for bloom
                ..default()
            },
            tonemapping: Tonemapping::TonyMcMapface, // 2. Using a tonemapper that desaturates to white is recommended
            ..default()
        },
        BloomSettings::default(), // 3. Enable bloom for the camera
    ));

    commands.spawn(SpriteBundle {
        texture: asset_server.load("bg.jpg"),
        transform: Transform::from_xyz(0.0, 20.0, 0.0),
        sprite: Sprite {
            custom_size: Some(vec2(2048.0 * 1.5 / 2.0, 2048.0 / 2.0)),
            ..default()
        },
        ..default()
    });
}

pub fn collisions_bullet_health(
    mut commands: Commands,
    mut events_try_damage: EventWriter<EventTryApplyDamages>,
    q_bullets: Query<(Entity, &Transform, &BulletOwner)>,
    q_health: Query<(Entity, &Transform, &Health)>,
) {
    for (e_bullet, bullet_position, bullet_owner) in q_bullets.iter() {
        for (e, t, _) in q_health.iter() {
            if bullet_owner.entity != e
                && bullet_position.translation.distance(t.translation) < 20f32
            {
                commands.entity(e_bullet).despawn();
                events_try_damage.send(EventTryApplyDamages(e, 0.25f32));
                continue;
            }
        }
    }
}
pub fn collisions_player_pickups(
    mut commands: Commands,
    q_pickups: Query<(Entity, &Transform, &HealthPickup)>,
    mut q_health: Query<(Entity, &Transform, &mut Health), Without<HealthPickup>>,
) {
    for (e, t, mut health) in q_health.iter_mut() {
        for (e_pickup, bullet_position, pickup) in q_pickups.iter() {
            if bullet_position.translation.distance(t.translation) < 20f32 {
                health.current += pickup.0;
                health.current = health.current.min(health.max);
                commands.entity(e_pickup).despawn();
                continue;
            }
        }
    }
}

pub fn try_apply_damages(
    mut commands: Commands,
    mut events_try_damage: EventReader<EventTryApplyDamages>,
    mut q_health: Query<(Entity, &Transform, &mut Health)>,
) {
    for ev in events_try_damage.iter() {
        let (e, transform, mut health) = q_health.get_mut(ev.0).unwrap();
        health.current -= 0.25f32;
        // TODO: fire event touched to spawn particles!
        if dbg!(health.current) <= 0f32 {
            commands.entity(e).despawn();
            commands.spawn((
                HealthPickup(0.1f32),
                Transform::from_translation(transform.translation),
            ));
        }
    }
}
