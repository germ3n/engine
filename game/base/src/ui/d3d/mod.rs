pub mod math;

#[cfg(windows)]
mod draw;
#[cfg(windows)]
mod shader;
#[cfg(windows)]
mod d3d9;
#[cfg(windows)]
mod d3d11;
#[cfg(windows)]
mod d3d12;

#[cfg(windows)]
pub use d3d9::D3D9Window;
#[cfg(windows)]
pub use d3d11::D3D11Window;
#[cfg(windows)]
pub use d3d12::D3D12Window;
