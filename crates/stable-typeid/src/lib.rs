use std::{
    any::type_name,
    hash::{Hash, Hasher},
};

pub trait TypeInfoExt {
    fn stable_type_id(&self) -> StableTypeId;
}

#[cfg_attr(features = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct StableTypeId {
    #[cfg(not(features = "uuid"))]
    id: u64,

    #[cfg(features = "uuid")]
    id: uuid::Uuid,

    #[cfg(test)]
    name: &'static str,
}

impl StableTypeId {
    pub fn of<T: ?Sized>() -> Self {
        #[cfg(not(features = "uuid"))]
        {
            let mut hasher = std::collections::hash_map::DefaultHasher::new();
            #[cfg(test)]
            let name = {
                let name = type_name::<T>();
                name.hash(&mut hasher);
                name
            };
            #[cfg(not(test))]
            type_name::<T>().hash(&mut hasher);

            let id = hasher.finish();
            Self {
                id,
                #[cfg(test)]
                name,
            }
        }
        #[cfg(features = "uuid")]
        {
            let name = type_name::<T>();
            Self {
                id: uuid::Uuid::new_v5(&uuid::Uuid::NAMESPACE_OID, name.as_bytes()),
                #[cfg(test)]
                name,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::marker::PhantomData;

    use super::*;

    struct Generic<T>(T);

    struct HasPhantom<T>(PhantomData<T>);

    #[test]
    fn hash_is_stable() {
        let id1 = StableTypeId::of::<u32>();
        let id2 = StableTypeId::of::<u32>();
        assert_eq!(id1.id, id2.id);
    }

    fn hashes_dont_collide<T, U>() {
        let id1 = StableTypeId::of::<T>();
        let id2 = StableTypeId::of::<U>();
        assert_ne!(
            id1.id, id2.id,
            "type ids are equal. type names: {} and {}",
            id1.name, id2.name
        );
    }

    #[test]
    #[should_panic]
    fn same_type_collides() {
        hashes_dont_collide::<u32, u32>();
    }

    #[test]
    fn generic_types_dont_collide() {
        hashes_dont_collide::<Generic<u32>, Generic<u64>>();
    }

    #[test]
    fn compose_phantom_wont_collide() {
        hashes_dont_collide::<HasPhantom<u32>, HasPhantom<u64>>();
    }
}
