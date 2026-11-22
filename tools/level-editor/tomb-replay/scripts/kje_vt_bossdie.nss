// The guardian of the tomb dies: the quest ends, then it dies as any creature does.
void main()
{
    AddJournalQuestEntry("kje_vessk", 40);
    ExecuteScript("k_def_death01", OBJECT_SELF);
}
