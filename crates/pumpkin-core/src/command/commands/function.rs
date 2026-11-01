use std::cell::Cell;

use pumpkin_data::translation;
use pumpkin_util::PermissionLvl;
use pumpkin_util::permission::{Permission, PermissionDefault, PermissionRegistry};
use pumpkin_util::text::TextComponent;

use crate::command::argument_builder::{ArgumentBuilder, argument, command};
use crate::command::argument_types::function::FunctionArgumentType;
use crate::command::context::command_context::CommandContext;
use crate::command::errors::command_syntax_error::CommandSyntaxError;
use crate::command::errors::error_types::{CommandErrorType, LiteralCommandErrorType};
use crate::command::node::dispatcher::CommandDispatcher;
use crate::command::node::{CommandExecutor, CommandExecutorResult};
use crate::command::suggestion::provider::{SuggestionProvider, SuggestionProviderResult};
use crate::command::suggestion::suggestions::SuggestionsBuilder;

const DESCRIPTION: &str = "Runs commands found in the corresponding function files.";
const PERMISSION: &str = "minecraft:command.function";

/// How deeply `/function` calls may nest before they are refused.
///
/// Vanilla feeds function lines through a command queue bounded by `maxCommandChainLength`
/// (`ExecutionContext#runCommandQueue`), so a function that never stops calling itself ends
/// there instead of growing the stack. Pumpkin runs function lines recursively, so the nesting
/// itself has to be bounded before it exhausts the thread stack.
const MAX_FUNCTION_DEPTH: usize = 64;

static ERROR_UNKNOWN_FUNCTION: CommandErrorType<1> = CommandErrorType::new(
    translation::java::ARGUMENTS_FUNCTION_UNKNOWN,
    translation::java::ARGUMENTS_FUNCTION_UNKNOWN,
);

static ERROR_FUNCTION_RECURSION: LiteralCommandErrorType =
    LiteralCommandErrorType::new("Function recursion limit reached");

thread_local! {
    static FUNCTION_DEPTH: Cell<usize> = const { Cell::new(0) };
}

/// Decrements the depth counter when the function call it guards returns.
struct FunctionDepthGuard;

impl FunctionDepthGuard {
    fn enter(function: &str) -> Result<Self, CommandSyntaxError> {
        FUNCTION_DEPTH.with(|depth| {
            if depth.get() >= MAX_FUNCTION_DEPTH {
                tracing::warn!(
                    "Refusing to run function '{function}': functions are nested deeper than {MAX_FUNCTION_DEPTH} levels"
                );
                return Err(ERROR_FUNCTION_RECURSION.create_without_context());
            }
            depth.set(depth.get() + 1);
            Ok(Self)
        })
    }
}

impl Drop for FunctionDepthGuard {
    fn drop(&mut self) {
        FUNCTION_DEPTH.with(|depth| depth.set(depth.get().saturating_sub(1)));
    }
}

struct FunctionSuggestionProvider;

impl SuggestionProvider for FunctionSuggestionProvider {
    fn suggest(
        &self,
        context: &CommandContext,
        mut builder: SuggestionsBuilder,
    ) -> SuggestionProviderResult {
        let server = context.server();
        let function_names = server.datapack_manager.get_function_names();
        for name in function_names {
            builder = builder.suggest(name);
        }
        builder.build()
    }
}

struct FunctionExecutor;

impl CommandExecutor for FunctionExecutor {
    fn execute(&self, context: &CommandContext) -> CommandExecutorResult {
        let name_str = FunctionArgumentType::get(context, "name")?;
        let server = context.server();

        let _depth_guard = FunctionDepthGuard::enter(name_str)?;

        let Ok(executed_count) =
            server
                .datapack_manager
                .execute_function(server, &context.source, name_str)
        else {
            return Err(ERROR_UNKNOWN_FUNCTION
                .create_without_context(TextComponent::text(name_str.to_string())));
        };

        if name_str.starts_with('#') {
            context.source.send_feedback(
                TextComponent::translate_cross(
                    translation::java::COMMANDS_FUNCTION_SUCCESS_MULTIPLE,
                    translation::java::COMMANDS_FUNCTION_SUCCESS_MULTIPLE,
                    [
                        TextComponent::text(executed_count.to_string()),
                        TextComponent::text(name_str.to_string()),
                    ],
                ),
                true,
            );
        } else {
            context.source.send_feedback(
                TextComponent::translate_cross(
                    translation::java::COMMANDS_FUNCTION_SUCCESS_SINGLE,
                    translation::java::COMMANDS_FUNCTION_SUCCESS_SINGLE,
                    [
                        TextComponent::text(executed_count.to_string()),
                        TextComponent::text(name_str.to_string()),
                    ],
                ),
                true,
            );
        }

        Ok(executed_count as i32)
    }
}

pub fn register(dispatcher: &mut CommandDispatcher, registry: &PermissionRegistry) {
    registry.register_permission_or_panic(Permission::new(
        PERMISSION,
        DESCRIPTION,
        PermissionDefault::Op(PermissionLvl::Two),
    ));

    dispatcher.register(
        command("function", DESCRIPTION).requires(PERMISSION).then(
            argument("name", FunctionArgumentType)
                .suggests(FunctionSuggestionProvider)
                .executes(FunctionExecutor),
        ),
    );
}
