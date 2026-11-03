// Reuse the same 2D artwork: large without video, compact beside a live camera.
window.gsProgramArt = {
  layout(holder, inset, footer, flat, sequenceFrame = null, showEquipment = false) {
    const destination = flat ? holder : footer;
    if (inset.parentElement !== destination) {
      if (flat) destination.appendChild(inset);
      else destination.insertBefore(inset, destination.firstChild);
    }
    holder.dataset.flat = String(flat);
    footer.dataset.rocketMain = String(flat);
    let equipment=inset.querySelector('#gs26-sequence-equipment');
    if(!equipment){equipment=inset.ownerDocument.createElement('section');equipment.id='gs26-sequence-equipment';equipment.setAttribute('aria-label','Programme GSE output evidence');inset.append(equipment);}
    equipment.hidden=!showEquipment;
    inset.dataset.equipment=String(showEquipment&&flat);
    window.GS26SequenceVisuals?.mount(equipment,sequenceFrame,{compact:!flat});
  }
};
