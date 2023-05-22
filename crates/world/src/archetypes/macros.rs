/// Generate an archetype with the given fields.
///
/// ```
/// def_archetype! {
///     <base_name>,
///     <field_name>: <field_type>,
///     ...
/// }
/// ```
///
/// Several structs will be generated:
/// - `<base_name>Archetype` - stores and owns the data.
/// - `<base_name>Ref` - a reference to a single item in the archetype.
/// - `<base_name>Builder` - a builder for spawning an item in the archetype.
/// - `<base_name>Iterator` - an iterator over the archetype data.
/// - `<base_name>Index` - an index into the archetype. Newtype over `u32`.
/// - `<base_name>Error` - an error type for the archetype.
///
/// This macro is intended to do the heavy lifting and ensure that there is a
/// single consistent list of fields that make up an archetype.
///
/// Functionality can be extended for any of the given structs by adding another
/// `impl` block.
///
/// e.g.
/// ```
/// def_archetype! {
///    Named,
///    name: String
/// }
///
/// impl NamedRef {
///     pub fn set_name_uppercase(&mut self) {
///         self.name = self.name.to_uppercase();
///    }
/// }
/// ```
#[macro_export]
macro_rules! def_archetype {
    ($base_name:ident, $($field:ident : $type:ty),*) => {
        paste::paste! {
            #[doc = "Generated archetype `" [<$base_name Archetype>] "`. See `def_archetype` macro.\n\n"]
            #[doc = "Fields contained within this archetype:\n"]
            $(#[doc = "- `" [< $field >] ": Vec<" [<$type>]">`\n"])*
            #[doc = "\nCo-generated with related types:\n"]
            #[doc = "- `" [<$base_name Ref>] "`\n"]
            #[doc = "- `" [<$base_name Builder>] "`\n"]
            #[doc = "- `" [<$base_name Iterator>] "`\n"]
            #[doc = "- `" [<$base_name Error>] "`\n"]
            #[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
            pub struct [<$base_name Archetype>] {
                /// Default builder for this archetype
                default_builder: Option<[<$base_name Builder>]>,
                despawned: bitvec::prelude::BitVec,
                next_index: usize,
                entities: Vec<u32>,
                $(
                    pub $field : Vec<$type>,
                )*
            }

            #[derive(Debug, Copy, Clone, Hash, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
            pub struct [<$base_name Index>](u32);

            impl From<u32> for [<$base_name Index>] {
                fn from(value: u32) -> Self {
                    Self(value)
                }
            }

            impl From<[<$base_name Index>]> for u32 {
                fn from(value: [<$base_name Index>]) -> Self {
                    value.0
                }
            }

            impl From<usize> for [<$base_name Index>] {
                fn from(value: usize) -> Self {
                    Self(value as u32)
                }
            }

            impl From<[<$base_name Index>]> for usize {
                fn from(value: [<$base_name Index>]) -> Self {
                    value.0 as usize
                }
            }

            #[doc = "Archetype `" [<$base_name Error>] "` error."]
            #[derive(thiserror::Error, Debug)]
            pub enum [<$base_name Error>] {
                #[error("builder is incomplete {builder:?}")]
                IncompleteBuilder { builder: [<$base_name Builder>] },
                #[error("entity not found {entity:?}")]
                EntityNotFound { entity: u32 },
            }

            impl Default for [<$base_name Archetype>] {
                fn default() -> Self {
                    Self {
                        next_index: 0,
                        default_builder: Default::default(),
                        entities: Vec::new(),
                        despawned: bitvec::prelude::BitVec::new(),
                        $(
                            $field : Vec::new(),
                        )*
                    }
                }
            }

            #[doc = "`" [<$base_name Ref>] "` builder for spawing an entity within `" [<$base_name Archetype>] "`."]
            #[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
            pub struct [<$base_name Builder>] {
                $(
                    $field: Option<$type>,
                )*
            }

            impl [<$base_name Builder>] {
                $(
                    #[doc = "Set the `" $field "` field in this builder."]
                    #[allow(dead_code)]
                    pub fn [<set_ $field>](&mut self, $field: $type) -> &mut Self {
                        self.$field = Some($field);
                        self
                    }
                )*
            }

            #[doc = "`" [<$base_name Ref>] "` - a reference to an entity within `" [<$base_name Archetype>] "`."]
            #[doc = "Fields are `pub` for easy access."]
            pub struct [<$base_name Ref>]<'a> {
                /// The entity id for this reference.
                pub entity_id: u32,
                $(
                    pub $field : &'a mut $type,
                )*
            }

            #[doc = "`" [<$base_name Ref>] "` iterator over entities within `" [<$base_name Archetype>] "`."]
            pub struct [<$base_name Iterator>]<'a> {
                entities: &'a [u32],
                despawned: &'a bitvec::prelude::BitVec,
                current_index: usize,
                $(
                    $field : core::slice::IterMut<'a, $type>,
                )*
            }

            impl<'a> Iterator for [<$base_name Iterator>]<'a> {
                type Item = [<$base_name Ref>]<'a>;
                fn next(&mut self) -> Option<Self::Item> {
                    // skip despawned entities.
                    let entity_id = *self.entities.get(self.current_index)?;
                    loop{
                        match self.despawned.get(self.current_index) {
                            Some(bit) if *bit => {
                                $(
                                    let _ = self.$field.next()?;
                                )*
                                self.current_index += 1;
                                continue;
                            }
                            _ => break
                        }
                    }
                    let n = Some([<$base_name Ref>] {
                        entity_id,
                        $(
                            $field : self.$field.next()?,
                        )*
                    });
                    self.current_index += 1;
                    n
                }
            }

            impl crate::Archetype for [<$base_name Archetype>] {
                type Error = [<$base_name Error>];
                type Builder = [<$base_name Builder>];
                type ItemRef<'a> = [<$base_name Ref>]<'a>;
                type IterMut<'a> = [<$base_name Iterator>]<'a>;

                #[doc = "Returns an iterator over the entities within `" [<$base_name Archetype>] "`."]
                fn iter_mut(&mut self) -> Self::IterMut<'_> {
                    [<$base_name Iterator>] {
                        entities: &self.entities,
                        despawned: &self.despawned,
                        current_index: 0usize,
                        $(
                            $field: self.$field.iter_mut(),
                        )*
                    }
                }

                #[doc = "Get a reference to an entity within `" [<$base_name Archetype>] "`."]
                fn get_mut(&mut self, entity_id: u32) -> Option<Self::ItemRef<'_>> {
                    let index: usize = self.entities.iter().position(|&e| e == entity_id)?;
                    match self.despawned.get(index) {
                        Some(b) if *b == true => return None,
                        _ => Some([<$base_name Ref>] {
                            entity_id,
                            $(
                                $field: self.$field.get_mut(index)?,
                            )*
                        })
                    }
                }

                #[doc = "Get a builder for spawning entities within `" [<$base_name Archetype>] "`. If a default builder is set "]
                fn builder(&self) -> Self::Builder {
                    match self.default_builder.clone() {
                        Some(builder) => builder,
                        // If there is no default builder, return a builder with all fields set to None.
                        None => Self::Builder {
                            $(
                                $field: <Option<$type>>::None,
                            )*
                        }
                    }
                }

                #[doc = "Set the default builder for this `" [<$base_name Archetype>] "`."]
                fn set_default_builder(&mut self, builder: Self::Builder) {
                    self.default_builder = Some(builder);
                }

                #[doc = "Spawn an entity within `" [<$base_name Archetype>] "`."]
                fn spawn(&mut self, entity: u32, builder: Self::Builder) -> Result<(), Self::Error> {
                    // reuse a slot if possible. Next index is still pointing to the end of the contiguous list.'
                    if let Some(empty_index) = self
                            .despawned
                            .iter()
                            .enumerate()
                            .filter_map(|(idx, b)| {
                                if *b == true { Some(idx) } else { None }
                            }).next() {
                        $(
                            self.$field[empty_index] = builder.$field.clone().unwrap();
                        )*
                        self.entities[empty_index] = entity;
                        self.despawned.set(empty_index, false);
                        return Ok(());
                    }
                    let result = match builder {
                        Self::Builder {
                            $(
                                $field: Some($field),
                            )*
                        } => {
                            $(
                                self.$field.push($field);
                            )*
                            self.entities.push(entity);
                            self.despawned.push(false);
                            Ok(())
                        }
                        _ => Err(Self::Error::IncompleteBuilder { builder }),
                    };
                    self.next_index += 1;
                    result
                }

                #[doc = "Despawn an entity within `" [<$base_name Archetype>] "`."]
                fn despawn(&mut self, entity: u32) -> Result<(), Self::Error> {
                    // Needs to be filled based on the actual logic of despawning.
                    let index = self.entities.iter().position(|&e| e == entity).ok_or_else(|| [<$base_name Error>]::EntityNotFound{entity})?;
                    self.despawned.set(index, true);
                    Ok(())
                }
            }
        }
    };
}

/// Define a reference and iterator over a subset of fields in an archetype.
#[macro_export]
macro_rules! def_ref_and_iter {
    ($base_name:ident, $($field:ident: $type:ty),*) => {
        paste::paste! {
            #[doc = "`" [<$base_name Ref>] "` a reference to fields within an archetype implementing an iterator with this type."]
            #[doc = "Fields are `pub` for easy access."]
            pub struct [<$base_name Ref>]<'a> {
                pub entity_id: u32,
                $(
                    pub $field : &'a mut $type,
                )*
            }

            #[doc = "`" [<$base_name Ref>] "` iterator over entities within and archetype implementing an iterator with this type."]
            pub struct [<$base_name Iterator>]<'a> {
                entities: &'a Vec<u32>,
                despawned: &'a bitvec::prelude::BitVec,
                current_index: usize,
                $(
                    $field : core::slice::IterMut<'a, $type>,
                )*
            }

            impl<'a> Iterator for [<$base_name Iterator>]<'a> {
                type Item = [<$base_name Ref>]<'a>;
                fn next(&mut self) -> Option<Self::Item> {
                    // skip despawned entities.
                    // could this break cache locality for the iteration?
                    let entity_id = *self.entities.get(self.current_index)?;
                    loop {
                        match self.despawned.get(self.current_index) {
                            Some(bit) if *bit => {
                                $(
                                    let _ = self.$field.next()?;
                                )*
                                self.current_index += 1;
                                continue;
                            },
                            _ => break                        }
                    }
                    let n = Some([<$base_name Ref>] {
                        entity_id,
                        $(
                            $field : self.$field.next()?,
                        )*
                    });
                    self.current_index += 1;
                    n
                }
            }
        }
    };
}

/// Implement an iterator method for an archetype with a type defined by
/// `def_ref_and_iter`.
#[macro_export]
macro_rules! impl_iter_method {
    ($base_name:ident => $archetype:ident, $($field:ident),*) => {
        paste::paste! {
            impl $archetype {
                #[doc = "Returns `" [<$base_name Iterator>] "<Item = "[<$base_name Ref>]">` over entities within `" $archetype "`."]
                #[allow(dead_code)]
                pub fn [<$base_name:snake _iter_mut>](&mut self) -> [<$base_name Iterator>]<'_> {
                    [<$base_name Iterator>] {
                        entities: &self.entities,
                        despawned: &self.despawned,
                        current_index: 0usize,
                        $(
                            $field: self.$field.iter_mut(),
                        )*
                    }
                }
            }
        }
    };
}

/// Implement `From<&mut $ref_name>` for `$builder`. Turns a reference in one
/// archetype into a builder for another.
#[macro_export]
macro_rules! impl_to_builder_for_ref {
    ($ref_name:ident < $builder:ident, $($field:ident),*) => {
        paste::paste! {
            impl<'a> From<&'a mut $ref_name<'a>> for $builder {
                fn from(value: &'a mut $ref_name<'a>) -> Self {
                    Self {
                        $(
                            $field: Some(value.$field.clone()),
                        )*
                    }
                }
            }
        }
    };
}

#[cfg(test)]
mod test {
    use glam::{Mat4, Vec3};

    use crate::graphics::{GfxIndex, Shape};
    use crate::health::HealthFacet;
    use crate::Archetype;

    // Defines:
    // BlobArchetype
    // BlobRef,
    // BlobBuilder,
    // BlobIterator,
    // BlobIndex,
    // BlobError
    def_archetype! {
        Blob,
        a_field: u32,
        b_field: u32,
        c_field: u32
    }

    def_ref_and_iter! {
        BAndC,
        b_field: u32,
        c_field: u32
    }

    impl_iter_method!(BAndC => BlobArchetype, b_field, c_field);

    def_archetype! {
        Blub,
        b_field: u32,
        c_field: u32
    }

    impl_iter_method! {
        BAndC => BlubArchetype,
        b_field,
        c_field
    }

    // compile error impl_to_builder_for_ref!(BAndCRef < BlobBuilder, b_field,
    // c_field)
    impl_to_builder_for_ref!(BAndCRef < BlubBuilder, b_field, c_field);

    // Add a method to BlobRef.
    impl<'r> BlobRef<'r> {
        fn add_one(&mut self) {
            *self.a_field += 1;
        }
    }

    def_archetype! {
        Blob2,
        a_field: u32
    }

    def_archetype! {
        Player,
        gfx: GfxIndex,
        position: Vec3,
        view: Mat4,
        perspective: Mat4,
        angles: Vec3,
        scale: f32,
        linear_velocity_intention: Vec3,
        angular_velocity_intention: Vec3,
        shape: Shape,
        health: HealthFacet
    }

    def_ref_and_iter! {
        HealthShape,
        shape: Shape,
        health: HealthFacet
    }

    impl_iter_method!(HealthShape => PlayerArchetype, shape, health);

    #[test]
    fn default_builder() {
        let mut blob_archetype: BlobArchetype = BlobArchetype::default();
        {
            let mut builder = blob_archetype.builder();
            assert_eq!(builder.a_field, None);

            builder.set_a_field(42);
            blob_archetype.set_default_builder(builder);
        }

        // builder() clones the default builder.
        let mut builder = blob_archetype.builder();
        builder.set_a_field(22);
        assert_eq!(builder.a_field, Some(22));

        // builder() still clones the set default builder.
        let another_builder = blob_archetype.builder();
        assert_eq!(another_builder.a_field, Some(42));
    }

    #[test]
    fn instantiate_archetype() {
        let mut blob_archetype: BlobArchetype = BlobArchetype::default();
        let mut blub_archetype: BlubArchetype = BlubArchetype::default();

        let mut builder = blob_archetype.builder();
        builder.set_a_field(42).set_b_field(22).set_c_field(33);
        blob_archetype.spawn(2, builder).unwrap();
        //blob_archetype.set_default_builder(builder.clone());

        let blob_iter = blob_archetype.iter_mut();
        for mut blob_ref in blob_iter {
            assert_eq!(*blob_ref.a_field, 42);
            *blob_ref.a_field = 43;
            blob_ref.add_one();
            assert_eq!(*blob_ref.a_field, 44);
        }

        let bc_iter = blob_archetype.b_and_c_iter_mut();

        let blub_bc_iter = blub_archetype
            .b_and_c_iter_mut()
            .map(|blub| *blub.b_field == 0);

        for bc_fields in bc_iter {
            *bc_fields.b_field += 1;
            *bc_fields.c_field = *bc_fields.b_field;
        }

        let blob_ref = blob_archetype.get_mut(2).unwrap();
        assert_eq!(*blob_ref.a_field, 44);
    }

    #[test]
    fn instantiate_error() {
        let blob_archetype: BlobArchetype = BlobArchetype::default();
        let builder = blob_archetype.builder();
        let err_str = format!(
            "{}",
            BlobError::IncompleteBuilder {
                builder: builder.clone()
            }
        );
        assert_eq!(
            err_str,
            "builder is incomplete BlobBuilder { a_field: None, b_field: None, c_field: None }"
        );
    }

    #[test]
    fn test_despawn() {
        let mut blob_archetype: BlobArchetype = BlobArchetype::default();
        let mut builder = blob_archetype.builder();
        builder.set_a_field(42).set_b_field(22).set_c_field(33);
        blob_archetype.spawn(22, builder.clone()).unwrap();
        blob_archetype.spawn(33, builder.clone()).unwrap();

        // despawn the first entity that
        blob_archetype.despawn(22).unwrap();

        assert_eq!(blob_archetype.next_index, 2usize);

        // despawned entity can't be reached
        assert!(blob_archetype.get_mut(22).is_none());

        // despawned entity can't be reached
        let i: Vec<BlobRef> = blob_archetype.iter_mut().collect();
        assert_eq!(i.len(), 1);

        // slot for despawned entity is reused
        blob_archetype.spawn(11, builder.clone()).unwrap();
        assert_eq!(
            blob_archetype.entities.iter().position(|&e| e == 11),
            Some(0)
        );
    }
}
