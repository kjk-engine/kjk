// The terminal took the three answers: the seal of the burial chamber opens.
void main()
{
    object oDoor = GetObjectByTag("kje_vt_seal");
    SetLocked(oDoor, FALSE);
    AssignCommand(oDoor, ActionOpenDoor(oDoor));
    AddJournalQuestEntry("kje_vessk", 30);
}
