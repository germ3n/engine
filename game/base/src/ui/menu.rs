use crate::ui::window::Window;
use crate::state::GameState;

pub fn draw_menu<T: Window, In, Out>(_window: &mut T, game: &mut GameState<In, Out>) {
    game.script_engine.run_hook("MenuPaint", ()); // render the menu
}