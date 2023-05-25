use std::collections::HashMap;

type Signature = [u8; 32];

pub struct DynamicArchetype {
    map: HashMap<Signature, Vec<Component>>,
}

pub struct DynamicArchetypeBuilder {
    fields: Vec<Component>,
}

impl DynamicArchetype {
    pub fn builder() -> DynamicArchetypeBuilder {
        DynamicArchetypeBuilder { fields: Vec::new() }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Hash)]
pub enum ComponentKey {
    Health,
    Physics,
    Draw,
    Camera,
    Player,
    Object,
}

pub enum Component {
    Health(HealthFacet),
    Physics(PhysicsFacet),
    Draw(DrawFacet),
    Camera(CameraFacet),
    Player(PlayerFacet),
    Object(ObjectFacet),
}
