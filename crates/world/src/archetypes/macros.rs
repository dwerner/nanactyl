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
/// def_archetype_boilerplate! {
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
            #[doc = "- `" [<$base_name Index>] "`\n"]
            #[doc = "- `" [<$base_name Error>] "`\n"]
            #[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
            pub struct [<$base_name Archetype>] {
                /// Default builder for this archetype
                default_builder: Option<[<$base_name Builder>]>,
                len: usize,
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
            }

            impl Default for [<$base_name Archetype>] {
                fn default() -> Self {
                    Self {
                        len: 0,
                        default_builder: Default::default(),
                        $(
                            $field : Vec::new(),
                        )*
                    }
                }
            }

            #[doc = "`" [<$base_name Ref>] "` builder for spawing an entity within `" [<$base_name Archetype>] "`."]
            #[derive(Debug, Default, Clone, serde::Serialize, serde::Deserialize)]
            pub struct [<$base_name Builder>] {
                $(
                    $field: Option<$type>,
                )*
            }

            impl [<$base_name Builder>] {
                $(
                    #[doc = "Set the `" $field "` field in this builder."]
                    #[allow(dead_code)]
                    pub fn [<set_ $field>](&mut self, $field: $type) {
                        self.$field = Some($field);
                    }
                )*
            }

            #[doc = "`" [<$base_name Ref>] "` - a reference to an entity within `" [<$base_name Archetype>] "`."]
            #[doc = "Fields are `pub` for easy access."]
            pub struct [<$base_name Ref>]<'a> {
                $(
                    pub $field : &'a mut $type,
                )*
            }


            #[doc = "`" [<$base_name Ref>] "` iterator over entities within `" [<$base_name Archetype>] "`."]
            pub struct [<$base_name Iterator>]<'a> {
                $(
                    $field : core::slice::IterMut<'a, $type>,
                )*
            }

            impl<'a> Iterator for [<$base_name Iterator>]<'a> {
                type Item = [<$base_name Ref>]<'a>;
                fn next(&mut self) -> Option<Self::Item> {
                    Some([<$base_name Ref>] {
                        $(
                            $field : self.$field.next()?,
                        )*
                    })
                }
            }

            impl crate::Archetype for [<$base_name Archetype>] {
                type Index = [<$base_name Index>];
                type Error = [<$base_name Error>];
                type Builder = [<$base_name Builder>];
                type ItemRef<'a> = [<$base_name Ref>]<'a>;
                type IterMut<'a> = [<$base_name Iterator>]<'a>;

                #[doc = "Returns the highest index of entities within `" [<$base_name Archetype>] "`."]
                fn len(&self) -> Self::Index {
                    self.len.into()
                }

                #[doc = "Returns an iterator over the entities within `" [<$base_name Archetype>] "`."]
                fn iter_mut(&mut self) -> Self::IterMut<'_> {
                    [<$base_name Iterator>] {
                        $(
                            $field: self.$field.iter_mut(),
                        )*
                    }
                }

                #[doc = "Get a reference to an entity within `" [<$base_name Archetype>] "`."]
                fn get_mut(&mut self, index: Self::Index) -> Option<Self::ItemRef<'_>> {
                    let index: usize = index.into();
                    Some([<$base_name Ref>] {
                        $(
                            $field: self.$field.get_mut(index)?,
                        )*
                    })
                }


                #[doc = "Get a builder for spawning entities within `" [<$base_name Archetype>] "`. If a default builder is set "]
                fn builder(&self) -> Self::Builder {
                    match self.default_builder.clone() {
                        Some(builder) => builder,
                        // If there is no default builder, return a builder with all fields set to None.
                        None => Self::Builder {
                            $(
                                $field: None,
                            )*
                        }
                    }
                }

                #[doc = "Set the default builder for this `" [<$base_name Archetype>] "`."]
                fn set_default_builder(&mut self, builder: Self::Builder) {
                    self.default_builder = Some(builder);
                }

                fn spawn(&mut self, builder: Self::Builder) -> Result<Self::Index, Self::Error> {
                    let result = match builder {
                        Self::Builder {
                            $(
                                $field: Some($field),
                            )*
                        } => {
                            $(
                                self.$field.push($field);
                            )*
                            Ok(self.len())
                        }
                        _ => Err(Self::Error::IncompleteBuilder { builder }),
                    };
                    self.len += 1;
                    result
                }

                fn despawn(&mut self, _index: Self::Index) -> Result<(), Self::Error> {
                    // Needs to be filled based on the actual logic of despawning.
                    unimplemented!()
                }
            }
        }
    };
}

/// Define a reference and iterator over a subset of fields in an archetype.
macro_rules! def_ref_and_iter {
    ($base_name:ident, $($field:ident: $type:ty),*) => {
        paste::paste! {
            #[doc = "`" [<$base_name Ref>] "` a reference to fields within an archetype implementing an iterator with this type."]
            #[doc = "Fields are `pub` for easy access."]
            pub struct [<$base_name Ref>]<'a> {
                $(
                    pub $field : &'a mut $type,
                )*
            }


            #[doc = "`" [<$base_name Ref>] "` iterator over entities within and archetype implementing an iterator with this type."]
            pub struct [<$base_name Iterator>]<'a> {
                $(
                    $field : core::slice::IterMut<'a, $type>,
                )*
            }

            impl<'a> Iterator for [<$base_name Iterator>]<'a> {
                type Item = [<$base_name Ref>]<'a>;
                fn next(&mut self) -> Option<Self::Item> {
                    Some([<$base_name Ref>] {
                        $(
                            $field : self.$field.next()?,
                        )*
                    })
                }
            }
        }
    };
}

/// Implement an Iterator and <base_name>Ref over a subset of fields in an
/// archetype. <base_name> is used to implement an accessor on the archetype (in
/// `snake_case`)
macro_rules! impl_archetype_ref_iter {
    ($base_name:ident => $archetype:ident, $($field:ident),*) => {
        paste::paste! {
            impl $archetype {
                #[doc = "Returns `" [<$base_name Iterator>] "<Item = "[<$base_name Ref>]">` over entities within `" $archetype "`."]
                #[allow(dead_code)]
                pub fn [<$base_name:snake _iter_mut>](&mut self) -> [<$base_name Iterator>]<'_> {
                    [<$base_name Iterator>] {
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

    use super::super::index::GfxIndex;
    use crate::thing::{HealthFacet, Shape};
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

    impl_archetype_ref_iter!(BAndC => BlobArchetype, b_field, c_field);

    def_archetype! {
        Blub,
        b_field: u32,
        c_field: u32
    }

    impl_archetype_ref_iter! {
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

    impl_archetype_ref_iter!(HealthShape => PlayerArchetype, shape, health);

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
        builder.set_a_field(42);
        let index = blob_archetype.spawn(builder).unwrap();
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
            .map(|blub| *blub.b_field = 0);

        for bc_fields in bc_iter {
            *bc_fields.b_field += 1;
            *bc_fields.c_field = *bc_fields.b_field;
        }

        let blob_ref = blob_archetype.get_mut(index).unwrap();
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
            "BlobArchetype builder is incomplete BlobBuilder { a_field: None }"
        );
    }
}
