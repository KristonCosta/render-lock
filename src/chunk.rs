use crate::mesh::MeshVertex;
use crate::worker::pool::Pool;
use crate::{ecs::component::*, worker::worker::Worker};
use legion::World;
use noise::Fbm;
use noise::{
    utils::{NoiseMapBuilder, PlaneMapBuilder},
    NoiseFn,
};
use std::thread;
use std::{
    collections::HashMap,
    sync::{mpsc, Arc, Mutex},
};

pub struct ChunkManager {
    current_idx: u32,
    receiver: mpsc::Receiver<MeshReference>,
    sender: mpsc::Sender<MeshReference>,
    pool: Pool<ChunkWork, ChunkWorkerInitializer, ChunkWorker>,
    pending: HashMap<u32, PendingWork>,
}

struct PendingWork {
    position: cgmath::Vector3<f32>,
    killer: mpsc::Sender<bool>,
}

impl PendingWork {
    fn kill(&mut self) {
        self.killer.send(true);
    }
}

impl ChunkManager {
    pub fn new(device: Arc<wgpu::Device>) -> Self {
        let (sender, receiver) = mpsc::channel();
        Self {
            current_idx: 1,
            receiver,
            sender,
            pool: Pool::new(
                1,
                ChunkWorkerInitializer {
                    device: Arc::clone(&device),
                },
            ),
            pending: HashMap::new(),
        }
    }

    pub fn dispatch(
        &mut self,
        position: cgmath::Vector3<f32>,
        chunk_location: cgmath::Vector2<f32>,
    ) {
        let (sender, receiver) = mpsc::channel();
        let pending_work = PendingWork {
            position,
            killer: sender,
        };
        let work = ChunkWork {
            idx: self.current_idx,
            position: chunk_location,
            receiver,
            sender: self.sender.clone(),
        };
        self.pending.insert(self.current_idx, pending_work);
        self.current_idx += 1;
        self.pool.dispatch(work);
    }

    pub fn update(&mut self, world: &mut World) {
        if let Ok(chunk) = self.receiver.try_recv() {
            let pending = self.pending.remove(&chunk.idx).unwrap();
            world.push((
                Transform {
                    position: pending.position,
                    rotation: cgmath::Euler::new(
                        cgmath::Rad(0.0),
                        cgmath::Rad(0.0),
                        cgmath::Rad(0.0),
                    ),
                },
                chunk,
            ));
        }
    }
}

pub struct ChunkWorker {
    id: usize,
    thread: thread::JoinHandle<()>,
}

pub struct ChunkWorkerInitializer {
    device: Arc<wgpu::Device>,
}

pub struct ChunkWork {
    idx: u32,
    position: cgmath::Vector2<f32>,
    receiver: mpsc::Receiver<bool>,
    sender: mpsc::Sender<MeshReference>,
}

impl Worker<ChunkWork, ChunkWorkerInitializer> for ChunkWorker {
    fn new(
        id: usize,
        bundle: &ChunkWorkerInitializer,
        receiver: Arc<Mutex<mpsc::Receiver<ChunkWork>>>,
    ) -> Self {
        let exector = ChunkExecutor {
            device: Arc::clone(&bundle.device),
        };
        let thread = thread::spawn(move || loop {
            let work = receiver.lock().unwrap().recv().unwrap();
            match work.receiver.try_recv() {
                Ok(true) => continue,
                Err(mpsc::TryRecvError::Disconnected) => continue,
                _ => {}
            }

            exector.execute(work);
        });

        Self { id, thread }
    }
}

pub struct ChunkExecutor {
    device: Arc<wgpu::Device>,
}

impl ChunkExecutor {
    fn execute(&self, data: ChunkWork) {
        let idx = data.idx;
        let mesh = make_mesh(idx as u32, data.position);
        data.sender.send(mesh).unwrap();
    }
}
const CHUNK_SIZE: usize = 32;

bitflags! {
    pub struct Sides: u32 {
        const NONE      = 0b00000000;
        const TOP       = 0b00000001;
        const BOTTOM    = 0b00000010;
        const LEFT      = 0b00000100;
        const RIGHT     = 0b00001000;
        const FORWARD   = 0b00010000;
        const BACKWARD  = 0b00100000;
    }
}

const CUBE_COORDINATES: [[f32; 3]; 8] = [
    [-0.5, -0.5, 0.5],
    [0.5, -0.5, 0.5],
    [0.5, -0.5, -0.5],
    [-0.5, -0.5, -0.5],
    [-0.5, 0.5, 0.5],
    [0.5, 0.5, 0.5],
    [0.5, 0.5, -0.5],
    [-0.5, 0.5, -0.5],
];

const UVS: [[f32; 2]; 4] = [
    [0.125, 1.0 - 0.9375],
    [0.1875, 1.0 - 0.9375],
    [0.125, 1.0 - 1.0],
    [0.1875, 1.0 - 1.0],
];

const QUAD_UV_ORDER: [u32; 4] = [3, 2, 0, 1];

pub struct ChunkBuilder {
    idx: u32,
}

impl ChunkBuilder {
    pub fn new() -> Self {
        Self { idx: 1 }
    }

    pub fn make_mesh(&mut self, chunk_location: cgmath::Vector2<f32>) -> MeshReference {
        let res = make_mesh(self.idx, chunk_location);
        self.idx += 1;
        res
    }
}

pub fn make_mesh(idx: u32, chunk_location: cgmath::Vector2<f32>) -> MeshReference {
    let mut builder = VoxelMeshBuilder::new();
    let mut fbm = Fbm::new();
    fbm.octaves = 4;
    fbm.persistence = 0.5;

    PlaneMapBuilder::new(&fbm).set_size(1000, 100);
    let mut height_map = vec![vec![vec![false; CHUNK_SIZE + 2]; CHUNK_SIZE + 2]; CHUNK_SIZE + 2];
    for x in 0..CHUNK_SIZE + 2 {
        for z in 0..CHUNK_SIZE + 2 {
            let stone_height = fbm.get([
                (x as f32 + chunk_location.x) as f64 * 0.05,
                (z as f32 + chunk_location.y) as f64 * 0.05,
            ]) * 16.0
                + (CHUNK_SIZE as f64 / 2.0);

            for y in 0..CHUNK_SIZE {
                height_map[x][y][z] = y < stone_height as usize;
            }
        }
    }

    for x in 1..CHUNK_SIZE + 1 {
        for y in 1..CHUNK_SIZE + 1 {
            for z in 1..CHUNK_SIZE + 1 {
                let pos = cgmath::Vector3::new(x, y, z);
                if height_map[x][y][z] {
                    builder
                        .set_position(&pos)
                        .generate_voxel(get_sides(&height_map, &pos));
                }
            }
        }
    }

    builder.build(idx)
}

fn get_sides(height_map: &Vec<Vec<Vec<bool>>>, pos: &cgmath::Vector3<usize>) -> Sides {
    let mut sides = Sides::NONE;

    if !(height_map[(pos.x - 1)][pos.y][pos.z]) {
        sides |= Sides::LEFT
    }

    if !(height_map[(pos.x + 1)][pos.y][pos.z]) {
        sides |= Sides::RIGHT
    }

    if (pos.y as i32) > 0 && !(height_map[(pos.x)][pos.y - 1][pos.z]) {
        sides |= Sides::BOTTOM
    }

    if !(height_map[(pos.x)][pos.y + 1][pos.z]) {
        sides |= Sides::TOP
    }

    if !(height_map[(pos.x)][pos.y][pos.z - 1]) {
        sides |= Sides::BACKWARD
    }

    if !(height_map[(pos.x)][pos.y][pos.z + 1]) {
        sides |= Sides::FORWARD
    }

    sides
}

pub struct VoxelMeshBuilder {
    current_cube_pos: cgmath::Vector3<u32>,
    indices: Vec<u32>,
    vertices: Vec<MeshVertex>,
    index_offset: u32,
}

const SIDE_VERTICES: [(Sides, [u32; 4]); 6] = [
    (Sides::TOP, [7, 6, 5, 4]),
    (Sides::BOTTOM, [0, 1, 2, 3]),
    (Sides::LEFT, [7, 4, 0, 3]),
    (Sides::RIGHT, [5, 6, 2, 1]),
    (Sides::FORWARD, [4, 5, 1, 0]),
    (Sides::BACKWARD, [6, 7, 3, 2]),
];

impl VoxelMeshBuilder {
    pub fn new() -> Self {
        Self {
            current_cube_pos: cgmath::Vector3::new(0, 0, 0),
            indices: Vec::new(),
            vertices: Vec::new(),
            index_offset: 0,
        }
    }

    pub fn set_position(&mut self, position: &cgmath::Vector3<usize>) -> &mut VoxelMeshBuilder {
        self.current_cube_pos.x = position.x as u32;
        self.current_cube_pos.y = position.y as u32;
        self.current_cube_pos.z = position.z as u32;
        self
    }

    pub fn move_position(&mut self, delta: cgmath::Vector3<u32>) -> &mut VoxelMeshBuilder {
        self.current_cube_pos += delta;
        self
    }

    pub fn generate_voxel(&mut self, sides: Sides) -> &mut VoxelMeshBuilder {
        for (side, indices) in SIDE_VERTICES.iter() {
            if sides.contains(*side) {
                self.build_quad(&indices);
            }
        }
        self
    }

    pub fn build(self, idx: u32) -> MeshReference {
        MeshReference {
            idx,
            vertex_data: self.vertices.into_boxed_slice(),
            index_data: self.indices.into_boxed_slice(),
        }
    }

    fn build_quad(&mut self, vertex_idx: &[u32; 4]) {
        for (vertex, uv) in vertex_idx.iter().zip(QUAD_UV_ORDER.iter()) {
            let mut v = CUBE_COORDINATES[*vertex as usize].clone();
            v[0] += self.current_cube_pos.x as f32;
            v[1] += self.current_cube_pos.y as f32;
            v[2] += self.current_cube_pos.z as f32;
            self.vertices.push(MeshVertex {
                position: v,
                tex_coords: UVS[*uv as usize].clone(),
                normal: [0.0, 1.0, 0.0],
            });
        }
        self.indices.extend(
            [
                3 + self.index_offset,
                1 + self.index_offset,
                0 + self.index_offset,
                3 + self.index_offset,
                2 + self.index_offset,
                1 + self.index_offset,
            ]
            .iter(),
        );
        self.index_offset += 4;
    }
}
