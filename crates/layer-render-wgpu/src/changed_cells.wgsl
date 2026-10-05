struct ChangedCells { side:u32, enabled:u32, padding0:u32,padding1:u32, cells:array<atomic<u32>> }
fn mark_changed_cell(pixel:vec2<u32>) {
    if changed_cells.enabled==0u {return;}
    let coordinate=pixel/changed_cells.side;
    atomicStore(&changed_cells.cells[coordinate.y*(256u/changed_cells.side)+coordinate.x],1u);
}
