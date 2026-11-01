use std::collections::HashMap;

use pumpkin_data::tag::{RegistryKey, get_tag_ids};
use pumpkin_data::{Block, BlockStateId, translation};
use pumpkin_nbt::compound::NbtCompound;
use pumpkin_nbt::tag::NbtTag;
use pumpkin_util::text::TextComponent;

use crate::argument_types::argument_type::{ArgumentType, JavaClientArgumentType};
use crate::argument_types::block::read_block;
use crate::argument_types::block_state::read_properties_with_defined;
use crate::argument_types::nbt::NbtCompoundArgumentType;
use crate::argument_types::nbt_path::compare_nbt;
use crate::context::command_context::CommandContext;
use crate::errors::command_syntax_error::CommandSyntaxError;
use crate::errors::error_types::CommandErrorType;
use crate::string_reader::StringReader;
use crate::suggestion::suggestions::{Suggestions, SuggestionsBuilder};

pub const ERROR_UNKNOWN_TAG: CommandErrorType<1> = CommandErrorType::new(
    translation::java::ARGUMENTS_BLOCK_TAG_UNKNOWN,
    translation::java::ARGUMENTS_BLOCK_TAG_UNKNOWN,
);

pub const ERROR_NO_VALUE: CommandErrorType<1> = CommandErrorType::new(
    translation::java::ARGUMENT_BLOCK_PROPERTY_NOVALUE,
    translation::java::ARGUMENT_BLOCK_PROPERTY_NOVALUE,
);

pub const ERROR_UNCLOSED_PROPERTIES: CommandErrorType<0> = CommandErrorType::new(
    translation::java::ARGUMENT_BLOCK_PROPERTY_UNCLOSED,
    translation::java::ARGUMENT_BLOCK_PROPERTY_UNCLOSED,
);

#[derive(Clone, Debug)]
pub enum BlockPredicate {
    Block {
        block: &'static Block,
        properties: HashMap<String, String>,
        nbt: Option<NbtCompound>,
    },
    Tag {
        tag_name: String,
        block_ids: Vec<u16>,
        properties: HashMap<String, String>,
        nbt: Option<NbtCompound>,
    },
}

impl BlockPredicate {
    /// Tests a block state; `block_entity_nbt` is only called when the predicate has NBT.
    #[must_use]
    pub fn test(
        &self,
        state_id: BlockStateId,
        block_entity_nbt: impl FnOnce() -> Option<NbtCompound>,
    ) -> bool {
        let block = Block::from_state_id(state_id);
        let (matches_block, properties, nbt) = match self {
            Self::Block {
                block: expected,
                properties,
                nbt,
            } => (block.id == expected.id, properties, nbt),
            Self::Tag {
                block_ids,
                properties,
                nbt,
                ..
            } => (block_ids.contains(&block.id.as_u16()), properties, nbt),
        };
        if !matches_block {
            return false;
        }

        if !properties.is_empty() {
            let Some(state_properties) = block.properties(state_id) else {
                return false;
            };
            let state_properties = state_properties.to_props();
            let all_match = properties.iter().all(|(key, value)| {
                state_properties
                    .iter()
                    .any(|p| p.key().to_str() == key && p.value_str() == value)
            });
            if !all_match {
                return false;
            }
        }

        nbt.as_ref().is_none_or(|expected| {
            block_entity_nbt().is_some_and(|actual| {
                compare_nbt(
                    &NbtTag::Compound(expected.clone()),
                    &NbtTag::Compound(actual),
                )
            })
        })
    }

    #[must_use]
    pub const fn requires_nbt(&self) -> bool {
        match self {
            Self::Block { nbt, .. } | Self::Tag { nbt, .. } => nbt.is_some(),
        }
    }
}

/// Parses `[property=value]` for a block tag. Vanilla reads these as vague properties
/// (`BlockStateParser.readVagueProperties`) and only rejects them while testing a block, when the
/// block state has no such property or value (vanilla's `BlockPredicateArgument.TagPredicate`).
fn parse_vague_properties(
    reader: &mut StringReader,
) -> Result<HashMap<String, String>, CommandSyntaxError> {
    let mut properties = HashMap::new();
    if reader.peek() == Some('[') {
        reader.skip();
        reader.skip_whitespace();
        while reader.can_read_char() && reader.peek() != Some(']') {
            let start_key = reader.cursor();
            while let Some(c) = reader.peek() {
                if c.is_alphanumeric() || c == '_' || c == '-' {
                    reader.skip();
                } else {
                    break;
                }
            }
            let key = reader.string()[start_key..reader.cursor()].to_string();
            reader.skip_whitespace();
            if reader.peek() != Some('=') {
                return Err(ERROR_NO_VALUE.create(reader, TextComponent::text(key)));
            }
            reader.skip();
            reader.skip_whitespace();
            let start_val = reader.cursor();
            while let Some(c) = reader.peek() {
                if c.is_alphanumeric() || c == '_' || c == '-' {
                    reader.skip();
                } else {
                    break;
                }
            }
            let val = reader.string()[start_val..reader.cursor()].to_string();
            properties.insert(key, val);
            reader.skip_whitespace();
            if reader.peek() == Some(',') {
                reader.skip();
                reader.skip_whitespace();
            } else if reader.peek() == Some(']') {
                break;
            } else {
                return Err(ERROR_UNCLOSED_PROPERTIES.create_without_context());
            }
        }
        if reader.peek() == Some(']') {
            reader.skip();
        } else {
            return Err(ERROR_UNCLOSED_PROPERTIES.create_without_context());
        }
    }
    Ok(properties)
}

fn parse_nbt(reader: &mut StringReader) -> Result<Option<NbtCompound>, CommandSyntaxError> {
    if reader.peek() == Some('{') {
        let tag =
            ArgumentType::<crate::source::DummySource>::parse(&NbtCompoundArgumentType, reader)?;
        Ok(Some(tag))
    } else {
        Ok(None)
    }
}

pub struct BlockPredicateArgumentType;

impl<S: crate::source::CommandSource> ArgumentType<S> for BlockPredicateArgumentType {
    type Item = BlockPredicate;

    fn parse(&self, reader: &mut StringReader) -> Result<Self::Item, CommandSyntaxError> {
        if reader.peek() == Some('#') {
            reader.skip();
            let start = reader.cursor();
            while let Some(c) = reader.peek() {
                if c.is_alphanumeric() || c == '_' || c == ':' || c == '/' || c == '.' || c == '-' {
                    reader.skip();
                } else {
                    break;
                }
            }
            let tag_str = &reader.string()[start..reader.cursor()];
            let normalized = if tag_str.contains(':') {
                tag_str.to_string()
            } else {
                format!("minecraft:{tag_str}")
            };
            let stripped = normalized.strip_prefix("minecraft:").unwrap_or(&normalized);

            let tag_ids = get_tag_ids(RegistryKey::Block, &normalized)
                .or_else(|| get_tag_ids(RegistryKey::Block, stripped));

            let block_ids = match tag_ids {
                Some(ids) => ids.to_vec(),
                None => {
                    return Err(ERROR_UNKNOWN_TAG.create(reader, TextComponent::text(normalized)));
                }
            };

            let properties = parse_vague_properties(reader)?;
            let nbt = parse_nbt(reader)?;

            Ok(BlockPredicate::Tag {
                tag_name: normalized,
                block_ids,
                properties,
                nbt,
            })
        } else {
            let block = read_block(reader)?;
            let (_, defined) = read_properties_with_defined(reader, block)?;
            let properties = defined
                .into_iter()
                .map(|property| {
                    (
                        property.key_str().to_string(),
                        property.value_str().to_string(),
                    )
                })
                .collect();
            let nbt = parse_nbt(reader)?;

            Ok(BlockPredicate::Block {
                block,
                properties,
                nbt,
            })
        }
    }

    fn client_side_parser(&self) -> JavaClientArgumentType {
        JavaClientArgumentType::BlockPredicate
    }

    fn examples(&self) -> Vec<String> {
        vec![
            "stone".to_string(),
            "minecraft:stone".to_string(),
            "stone[foo=bar]".to_string(),
            "#stone".to_string(),
            "#stone[foo=bar]{baz=nbt}".to_string(),
        ]
    }

    fn list_suggestions(
        &self,
        _context: &CommandContext<S>,
        builder: SuggestionsBuilder,
    ) -> Suggestions {
        builder.build()
    }
}

impl BlockPredicateArgumentType {
    pub fn get<S: crate::source::CommandSource>(
        context: &CommandContext<S>,
        name: &str,
    ) -> Result<BlockPredicate, CommandSyntaxError> {
        Ok(context.get_argument::<BlockPredicate>(name)?.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::argument_types::block_state::{ERROR_INVALID_VALUE, ERROR_UNKNOWN_PROPERTY};
    use crate::source::DummySource;
    use pumpkin_data::block_properties::{SlabProperties, SlabType};

    fn parse(input: &str) -> Result<BlockPredicate, CommandSyntaxError> {
        let mut reader = StringReader::new(input);
        ArgumentType::<DummySource>::parse(&BlockPredicateArgumentType, &mut reader)
    }

    #[test]
    fn invalid_properties_are_rejected() {
        assert!(
            parse("oak_slab[sideways=up]")
                .unwrap_err()
                .is(&ERROR_UNKNOWN_PROPERTY)
        );
        assert!(
            parse("oak_slab[type=sideways]")
                .unwrap_err()
                .is(&ERROR_INVALID_VALUE)
        );
    }

    #[test]
    fn only_the_given_properties_have_to_match() {
        let top = SlabProperties {
            r#type: SlabType::Top,
            r#waterlogged: false,
        }
        .to_state_id(&Block::OAK_SLAB);
        let bottom = SlabProperties {
            r#type: SlabType::Bottom,
            r#waterlogged: false,
        }
        .to_state_id(&Block::OAK_SLAB);

        let any_slab = parse("oak_slab").unwrap();
        assert!(any_slab.test(top, || None));
        assert!(any_slab.test(bottom, || None));

        let top_slab = parse("oak_slab[type=top]").unwrap();
        assert!(top_slab.test(top, || None));
        assert!(!top_slab.test(bottom, || None));
    }
}
