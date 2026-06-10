import type { Contact, CreateContactInput } from "../types/mail"
import { tauriInvoke } from "./tauri"

export async function listContacts(search?: string): Promise<Contact[]> {
  return tauriInvoke<Contact[]>("list_contacts", { search: search ?? null })
}

export async function createContact(input: CreateContactInput): Promise<Contact> {
  return tauriInvoke<Contact>("create_contact", { input })
}

export async function updateContact(id: string, input: CreateContactInput): Promise<Contact> {
  return tauriInvoke<Contact>("update_contact", { id, input })
}

export async function deleteContact(id: string): Promise<void> {
  await tauriInvoke<void>("delete_contact", { id })
}

export async function importContacts(): Promise<number> {
  return tauriInvoke<number>("import_contacts")
}
