use crate::ui::window::Window;
use crate::GameState;

pub fn draw_menu<T: Window>(_window: &mut T, game: &mut GameState) {
    game.script_engine.run_hook("MenuPaint", ()); // render the menu
}