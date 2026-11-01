use pumpkin_data::{Block, BlockStateId, Property, translation};
use pumpkin_util::resource_location::ToResourceLocation;
use pumpkin_util::text::TextComponent;

use crate::argument_types::argument_type::{ArgumentType, JavaClientArgumentType};
use crate::argument_types::block::read_block;
use crate::context::command_context::CommandContext;
use crate::errors::command_syntax_error::CommandSyntaxError;
use crate::errors::error_types::CommandErrorType;
use crate::string_reader::StringReader;
use crate::suggestion::suggestions::{Suggestions, SuggestionsBuilder};

pub const ERROR_NO_TAGS_ALLOWED: CommandErrorType<0> = CommandErrorType::new(
    translation::java::ARGUMENT_BLOCK_TAG_DISALLOWED,
    translation::java::ARGUMENT_BLOCK_TAG_DISALLOWED,
);

pub const ERROR_UNKNOWN_PROPERTY: CommandErrorType<2> = CommandErrorType::new(
    translation::java::ARGUMENT_BLOCK_PROPERTY_UNKNOWN,
    translation::java::ARGUMENT_BLOCK_PROPERTY_UNKNOWN,
);

pub const ERROR_DUPLICATE_PROPERTY: CommandErrorType<2> = CommandErrorType::new(
    translation::java::ARGUMENT_BLOCK_PROPERTY_DUPLICATE,
    translation::java::ARGUMENT_BLOCK_PROPERTY_DUPLICATE,
);

pub const ERROR_INVALID_VALUE: CommandErrorType<3> = CommandErrorType::new(
    translation::java::ARGUMENT_BLOCK_PROPERTY_INVALID,
    translation::java::ARGUMENT_BLOCK_PROPERTY_INVALID,
);

pub const ERROR_EXPECTED_VALUE: CommandErrorType<2> = CommandErrorType::new(
    translation::java::ARGUMENT_BLOCK_PROPERTY_NOVALUE,
    translation::java::ARGUMENT_BLOCK_PROPERTY_NOVALUE,
);

pub const ERROR_EXPECTED_END_OF_PROPERTIES: CommandErrorType<0> = CommandErrorType::new(
    translation::java::ARGUMENT_BLOCK_PROPERTY_UNCLOSED,
    translation::java::ARGUMENT_BLOCK_PROPERTY_UNCLOSED,
);

const SYNTAX_START_PROPERTIES: char = '[';
const SYNTAX_END_PROPERTIES: char = ']';
const SYNTAX_EQUALS: char = '=';
const SYNTAX_PROPERTY_SEPARATOR: char = ',';
const SYNTAX_TAG: char = '#';

/// Parses a block together with optional state properties, such as `oak_slab[type=top]`.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub struct BlockStateArgumentType;

impl<S: crate::source::CommandSource> ArgumentType<S> for BlockStateArgumentType {
    type Item = BlockStateId;

    fn parse(&self, reader: &mut StringReader) -> Result<Self::Item, CommandSyntaxError> {
        // Vanilla's parser only accepts tags when it is parsing a predicate.
        if reader.peek() == Some(SYNTAX_TAG) {
            return Err(ERROR_NO_TAGS_ALLOWED.create(reader));
        }

        let block = read_block(reader)?;
        read_properties(reader, block)
    }

    fn client_side_parser(&'_ self) -> JavaClientArgumentType {
        JavaClientArgumentType::BlockState
    }

    fn examples(&self) -> Vec<String> {
        vec![
            "stone".to_string(),
            "minecraft:stone".to_string(),
            "oak_slab[type=top]".to_string(),
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

impl_copy_get!(BlockStateArgumentType, BlockStateId);

/// Port of vanilla's `BlockStateParser.readProperties`.
fn read_properties(
    reader: &mut StringReader,
    block: &'static Block,
) -> Result<BlockStateId, CommandSyntaxError> {
    read_properties_with_defined(reader, block).map(|(state_id, _)| state_id)
}

/// `read_properties`'s parse, also returning the properties the input sets. A predicate only
/// compares those, which is what vanilla's `BlockStateParser.BlockResult.properties` carries.
pub fn read_properties_with_defined(
    reader: &mut StringReader,
    block: &'static Block,
) -> Result<(BlockStateId, Vec<Property>), CommandSyntaxError> {
    if reader.peek() != Some(SYNTAX_START_PROPERTIES) {
        return Ok((block.default_state.id, Vec::new()));
    }

    // Vanilla starts from the default state and only overrides the properties it reads.
    let mut properties = block
        .properties(block.default_state.id)
        .map_or_else(Vec::new, |properties| properties.to_props());
    let mut defined_keys = Vec::new();
    let mut defined = Vec::new();

    reader.skip();
    reader.skip_whitespace();

    while reader.can_read_char() && reader.peek() != Some(SYNTAX_END_PROPERTIES) {
        reader.skip_whitespace();
        let key_start = reader.cursor();
        let key = reader.read_string()?;

        let Some(index) = properties
            .iter()
            .position(|property| property.key_str() == key)
        else {
            reader.set_cursor(key_start);
            return Err(ERROR_UNKNOWN_PROPERTY.create(
                reader,
                block_id(block),
                TextComponent::text(key),
            ));
        };

        // Vanilla passes the property before the block for this error and the next one.
        if defined_keys.contains(&properties[index].key()) {
            reader.set_cursor(key_start);
            return Err(ERROR_DUPLICATE_PROPERTY.create(
                reader,
                TextComponent::text(key),
                block_id(block),
            ));
        }
        defined_keys.push(properties[index].key());

        reader.skip_whitespace();
        if reader.peek() != Some(SYNTAX_EQUALS) {
            return Err(ERROR_EXPECTED_VALUE.create(
                reader,
                TextComponent::text(key),
                block_id(block),
            ));
        }

        reader.skip();
        reader.skip_whitespace();
        let value_start = reader.cursor();
        let value = reader.read_string()?;
        let Some(property) = find_state_property(block, &key, &value) else {
            reader.set_cursor(value_start);
            return Err(ERROR_INVALID_VALUE.create(
                reader,
                block_id(block),
                TextComponent::text(value),
                TextComponent::text(key),
            ));
        };
        properties[index] = property;
        defined.push(property);

        reader.skip_whitespace();
        if reader.can_read_char() {
            if reader.peek() != Some(SYNTAX_PROPERTY_SEPARATOR) {
                if reader.peek() != Some(SYNTAX_END_PROPERTIES) {
                    return Err(ERROR_EXPECTED_END_OF_PROPERTIES.create(reader));
                }
                break;
            }
            reader.skip();
        }
    }

    if reader.can_read_char() {
        reader.skip();
    } else {
        return Err(ERROR_EXPECTED_END_OF_PROPERTIES.create(reader));
    }

    Ok((
        block.from_properties(&properties).to_state_id(block),
        defined,
    ))
}

/// Looks up the property a block state actually holds, which is what validates the value:
/// vanilla's `Property.getValue` only knows the property type, so values that exist for the
/// type but not for this block (such as `age=9` on wheat) have to be rejected here.
fn find_state_property(block: &'static Block, key: &str, value: &str) -> Option<Property> {
    block.states.iter().find_map(|state| {
        let properties = block.properties(state.id)?;
        properties
            .to_props()
            .into_iter()
            .find(|property| property.key_str() == key && property.value_str() == value)
    })
}

fn block_id(block: &'static Block) -> TextComponent {
    TextComponent::text(block.to_resource_location())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::source::DummySource;
    use pumpkin_data::block_properties::{SlabProperties, SlabType};

    fn parse(input: &str) -> Result<BlockStateId, CommandSyntaxError> {
        let mut reader = StringReader::new(input);
        ArgumentType::<DummySource>::parse(&BlockStateArgumentType, &mut reader)
    }

    #[test]
    fn properties_are_applied_to_the_block() {
        let state_id = parse("minecraft:oak_slab[type=top]").unwrap();
        let properties = SlabProperties::from_state_id(state_id);

        assert_eq!(Block::from_state_id(state_id), &Block::OAK_SLAB);
        assert_eq!(properties.r#type, SlabType::Top);
        assert_ne!(state_id, Block::OAK_SLAB.default_state.id);

        // Unset properties keep their default value, like vanilla's `BlockState.setValue` chain.
        let state_id = parse("oak_slab[waterlogged=true]").unwrap();
        let properties = SlabProperties::from_state_id(state_id);
        assert!(properties.r#waterlogged);
        assert_eq!(properties.r#type, SlabType::Bottom);
    }

    #[test]
    fn invalid_properties_are_rejected() {
        assert!(
            parse("oak_slab[type=sideways]")
                .unwrap_err()
                .is(&ERROR_INVALID_VALUE)
        );
        assert!(
            parse("stone[type=top]")
                .unwrap_err()
                .is(&ERROR_UNKNOWN_PROPERTY)
        );
        assert!(
            parse("oak_slab[type=top,type=bottom]")
                .unwrap_err()
                .is(&ERROR_DUPLICATE_PROPERTY)
        );
        assert!(
            parse("oak_slab[type=top")
                .unwrap_err()
                .is(&ERROR_EXPECTED_END_OF_PROPERTIES)
        );
        assert!(
            parse("oak_slab[type]")
                .unwrap_err()
                .is(&ERROR_EXPECTED_VALUE)
        );
        assert!(parse("#oak_slabs").unwrap_err().is(&ERROR_NO_TAGS_ALLOWED));
    }
}
